pub mod plugins;
pub mod security;

use base64::Engine as B64Engine;
use nu_cmd_lang::create_default_context;
use nu_engine::eval_block_with_early_return;
use nu_parser::parse;
use nu_protocol::debugger::WithoutDebug;
use nu_protocol::engine::{EngineState, Stack, StateWorkingSet};
use nu_protocol::{PipelineData, Record, ShellError, Span, Value as NuValue};
use security::{SecurityError, SecurityPolicy};
use serde_json::{Map, Value as JsonValue};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// Whether a task failure could plausibly succeed on a re-dispatch.
///
/// `Deterministic` means the script itself is wrong for this input (type mismatch, missing
/// column, empty-aggregation `UnsupportedInput`, …): re-running burns tokens and a full
/// journey-context seed to fail identically, so the auto-dispatch retry gate stops after one
/// attempt. `Transient` keeps the full retry budget. Unknown shapes MUST default to
/// `Transient` — a misclassified conduit 5xx would strand a healthy step, while a
/// misclassified script bug merely burns the bounded retry budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NuErrorClass {
    Deterministic,
    Transient,
}

impl NuErrorClass {
    pub fn as_str(&self) -> &'static str {
        match self {
            NuErrorClass::Deterministic => "deterministic",
            NuErrorClass::Transient => "transient",
        }
    }
}

#[derive(Debug)]
pub enum NuError {
    Security(SecurityError),
    Parse(String),
    Runtime {
        message: String,
        class: NuErrorClass,
    },
    ContractViolation(String),
}

impl NuError {
    pub fn error_class(&self) -> NuErrorClass {
        match self {
            // A parse error, a security rejection, or a contract-shape violation cannot be
            // fixed by re-running the same script version.
            NuError::Security(_) | NuError::Parse(_) | NuError::ContractViolation(_) => {
                NuErrorClass::Deterministic
            }
            NuError::Runtime { class, .. } => *class,
        }
    }
}

impl std::fmt::Display for NuError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NuError::Security(e) => write!(f, "Security error: {}", e),
            NuError::Parse(e) => write!(f, "Parse error: {}", e),
            NuError::Runtime { message, .. } => write!(f, "Runtime error: {}", message),
            NuError::ContractViolation(e) => write!(f, "Contract violation: {}", e),
        }
    }
}

impl std::error::Error for NuError {}

impl From<SecurityError> for NuError {
    fn from(e: SecurityError) -> Self {
        NuError::Security(e)
    }
}

/// Classify a `ShellError` by variant. Language-semantics failures (the script disagrees
/// with its input's shape) are deterministic; everything a custom command raises is
/// `GenericError` (all 146 construction sites in `commands/`), which covers conduit, EDGAR,
/// and network failures and therefore stays transient. `EvalBlockWithInput` is a wrapper
/// (e.g. a failure inside an `each` closure): any deterministic source makes the whole
/// error deterministic.
fn classify_shell_error(e: &ShellError) -> NuErrorClass {
    match e {
        ShellError::EvalBlockWithInput { sources, .. } => {
            if sources
                .iter()
                .any(|s| classify_shell_error(s) == NuErrorClass::Deterministic)
            {
                NuErrorClass::Deterministic
            } else {
                NuErrorClass::Transient
            }
        }
        ShellError::OperatorUnsupportedType { .. }
        | ShellError::OperatorIncompatibleTypes { .. }
        | ShellError::PipelineMismatch { .. }
        | ShellError::OnlySupportsThisInputType { .. }
        | ShellError::PipelineEmpty { .. }
        | ShellError::TypeMismatch { .. }
        | ShellError::RuntimeTypeMismatch { .. }
        | ShellError::InvalidValue { .. }
        | ShellError::IncorrectValue { .. }
        | ShellError::UnknownOperator { .. }
        | ShellError::MissingParameter { .. }
        | ShellError::IncompatibleParameters { .. }
        | ShellError::IncompatibleParametersSingle { .. }
        | ShellError::InvalidRange { .. }
        | ShellError::VariableNotFoundAtRuntime { .. }
        | ShellError::EnvVarNotFoundAtRuntime { .. }
        | ShellError::CantConvert { .. }
        | ShellError::CantConvertToUnit { .. }
        | ShellError::DivisionByZero { .. }
        | ShellError::CannotCreateRange { .. }
        | ShellError::AccessBeyondEnd { .. }
        | ShellError::AccessEmptyContent { .. }
        | ShellError::AccessBeyondEndOfStream { .. }
        | ShellError::IncompatiblePathAccess { .. }
        | ShellError::CantFindColumn { .. }
        | ShellError::ColumnAlreadyExists { .. }
        | ShellError::NotAList { .. }
        | ShellError::RecordColsValsMismatch { .. }
        | ShellError::UnsupportedInput { .. }
        | ShellError::DatetimeParseError { .. }
        | ShellError::CommandNotFound { .. }
        | ShellError::AliasNotFound { .. }
        | ShellError::DidYouMean { .. }
        | ShellError::DidYouMeanCustom { .. }
        | ShellError::MissingRequiredColumn { .. }
        | ShellError::NeedsPositiveValue { .. }
        | ShellError::MatchGuardNotBool { .. } => NuErrorClass::Deterministic,
        _ => NuErrorClass::Transient,
    }
}

impl From<ShellError> for NuError {
    fn from(e: ShellError) -> Self {
        NuError::Runtime {
            class: classify_shell_error(&e),
            message: format!("{:?}", e),
        }
    }
}

#[derive(Debug)]
pub struct NuOutput {
    pub payload: JsonValue,
    pub signals: HashMap<String, JsonValue>,
}

pub struct StepEngine {
    engine_state: EngineState,
    error_cache: HashMap<[u8; 32], String>,
    security_policy: SecurityPolicy,
}

impl StepEngine {
    pub fn new() -> Self {
        Self::with_policy(SecurityPolicy::default())
    }

    pub fn with_policy(policy: SecurityPolicy) -> Self {
        // Create default nushell context with core language commands
        let engine_state = create_default_context();

        // Register standard commands from nu-command (str, each, where, math, etc.)
        let mut engine_state = nu_command::add_shell_command_context(engine_state);

        // Register custom dapple commands (prompt, prompt send, etc.)
        crate::commands::register_dapple_commands(&mut engine_state);

        Self {
            engine_state,
            error_cache: HashMap::new(),
            security_policy: policy,
        }
    }

    fn hash_script(script: &str) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(script.as_bytes());
        hasher.finalize().into()
    }

    /// Parse and validate a nushell script without executing it.
    ///
    /// Checks for parse errors and security policy violations.
    /// Does not modify engine state or execute the script.
    pub fn validate_script(&self, script: &str) -> Result<(), NuError> {
        let mut working_set = StateWorkingSet::new(&self.engine_state);
        let block = parse(&mut working_set, None, script.as_bytes(), false);

        if !working_set.parse_errors.is_empty() {
            let errors: Vec<String> = working_set
                .parse_errors
                .iter()
                .map(|e| e.to_string())
                .collect();
            return Err(NuError::Parse(errors.join("; ")));
        }

        self.security_policy.validate_ast(&block, &working_set)?;
        Ok(())
    }

    // Borrows its input: the caller's context tree (a multi-MB journey seed on
    // late-stage journeys) is converted straight to a nu Value without an
    // intermediate owned copy.
    pub fn execute_script(
        &mut self,
        script: &str,
        input: &JsonValue,
        metadata: Option<&JsonValue>,
    ) -> Result<NuOutput, NuError> {
        let hash = Self::hash_script(script);

        // Check error cache first - return cached error immediately
        if let Some(cached_error) = self.error_cache.get(&hash) {
            return Err(NuError::Parse(cached_error.clone()));
        }

        // Parse the script - closures create blocks that must be merged into engine state
        let mut working_set = StateWorkingSet::new(&self.engine_state);
        let block = parse(&mut working_set, None, script.as_bytes(), false);

        // Check for parse errors
        if !working_set.parse_errors.is_empty() {
            let errors: Vec<String> = working_set
                .parse_errors
                .iter()
                .map(|e| e.to_string())
                .collect();
            self.error_cache.insert(hash, errors.join("; "));
            return Err(NuError::Parse(errors.join("; ")));
        }

        // Validate against security policy. The working set is what makes the
        // closure / subexpression bodies reachable — without it the walk sees
        // only top-level pipeline elements.
        self.security_policy.validate_ast(&block, &working_set)?;

        // Merge the working set delta into engine state - this makes closure blocks available
        let _ = self.engine_state.merge_delta(working_set.render());

        // Execute the block
        self.execute_block(&block, input, metadata)
    }

    fn execute_block(
        &self,
        block: &nu_protocol::ast::Block,
        input: &JsonValue,
        metadata: Option<&JsonValue>,
    ) -> Result<NuOutput, NuError> {
        // Create a stack for execution
        let mut stack = Stack::new();

        // Set PWD — some nushell commands internally require $env.PWD
        let pwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("/tmp"));
        stack.add_env_var(
            "PWD".to_string(),
            NuValue::string(pwd.to_string_lossy(), Span::unknown()),
        );

        // For nu 0.100, we inject input via the pipeline, not as a variable
        // Build input data as a record if it's an object
        let input_nu = json_to_nu_value(input);

        // Inject $METADATA if provided - we'll use environment variables
        if let Some(meta) = metadata {
            let size = serde_json::to_string(meta).map(|s| s.len()).unwrap_or(0);
            if size > self.security_policy.max_metadata_size() {
                return Err(NuError::Security(SecurityError::DisallowedCommand(
                    format!(
                        "Metadata size {} exceeds limit {}",
                        size,
                        self.security_policy.max_metadata_size()
                    ),
                )));
            }
            let meta_nu = json_to_nu_value(meta);
            stack.add_env_var("DAPPLE_METADATA".to_string(), meta_nu);
        }

        // Input reaches the script as `$in` (the pipeline) ONLY. A `DAPPLE_INPUT`
        // env alias used to clone the entire context tree here — a second full
        // copy of a multi-MB journey seed that no script ever read.
        let pipeline_input = PipelineData::Value(input_nu, None);
        let result = eval_block_with_early_return::<WithoutDebug>(
            &self.engine_state,
            &mut stack,
            block,
            pipeline_input,
        )?;

        // Extract result - access .body from PipelineExecutionData
        let output_value = result.body.into_value(Span::unknown())?;
        let output_json = nu_value_to_json(&output_value);

        // Validate contract: must be { payload, signals }
        self.validate_and_extract_contract(output_json)
    }

    fn validate_and_extract_contract(&self, value: JsonValue) -> Result<NuOutput, NuError> {
        let obj = value
            .as_object()
            .ok_or_else(|| NuError::ContractViolation("Script must return a record".to_string()))?;

        let payload = obj.get("payload").cloned().ok_or_else(|| {
            NuError::ContractViolation(
                "Script must return a record with 'payload' field".to_string(),
            )
        })?;

        let signals = obj
            .get("signals")
            .cloned()
            .unwrap_or(JsonValue::Object(Map::new()));
        let signals_obj = signals.as_object().ok_or_else(|| {
            NuError::ContractViolation("'signals' field must be a record".to_string())
        })?;

        let signals_map: HashMap<String, JsonValue> = signals_obj
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        Ok(NuOutput {
            payload,
            signals: signals_map,
        })
    }
}

impl Default for StepEngine {
    fn default() -> Self {
        Self::new()
    }
}

pub fn json_to_nu_value(json: &JsonValue) -> NuValue {
    match json {
        JsonValue::Null => NuValue::nothing(Span::unknown()),
        JsonValue::Bool(b) => NuValue::bool(*b, Span::unknown()),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                NuValue::int(i, Span::unknown())
            } else if let Some(f) = n.as_f64() {
                NuValue::float(f, Span::unknown())
            } else {
                NuValue::string(n.to_string(), Span::unknown())
            }
        }
        JsonValue::String(s) => NuValue::string(s.clone(), Span::unknown()),
        JsonValue::Array(arr) => {
            let vals: Vec<NuValue> = arr.iter().map(json_to_nu_value).collect();
            NuValue::list(vals, Span::unknown())
        }
        JsonValue::Object(obj) => {
            let mut record = Record::new();
            for (k, v) in obj {
                record.push(k.clone(), json_to_nu_value(v));
            }
            NuValue::record(record, Span::unknown())
        }
    }
}

pub fn nu_value_to_json(value: &NuValue) -> JsonValue {
    match value {
        NuValue::Nothing { .. } => JsonValue::Null,
        NuValue::Bool { val, .. } => JsonValue::Bool(*val),
        NuValue::Int { val, .. } => JsonValue::Number((*val).into()),
        NuValue::Float { val, .. } => serde_json::Number::from_f64(*val)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
        NuValue::String { val, .. } => JsonValue::String(val.clone()),
        NuValue::List { vals, .. } => {
            let arr: Vec<JsonValue> = vals.iter().map(nu_value_to_json).collect();
            JsonValue::Array(arr)
        }
        NuValue::Record { val, .. } => {
            let obj: Map<String, JsonValue> = val
                .iter()
                .map(|(k, v)| (k.clone(), nu_value_to_json(v)))
                .collect();
            JsonValue::Object(obj)
        }
        NuValue::Binary { val, .. } => {
            JsonValue::String(base64::engine::general_purpose::STANDARD.encode(val))
        }
        NuValue::Date { val, .. } => JsonValue::String(val.to_rfc3339()),
        NuValue::Duration { val, .. } => JsonValue::Number((*val).into()),
        NuValue::Filesize { val, .. } => JsonValue::Number(val.get().into()),
        NuValue::Range { val, .. } => {
            // Serialize range as a description string
            JsonValue::String(format!("{:?}", val))
        }
        NuValue::Closure { .. } => JsonValue::String("<closure>".to_string()),
        NuValue::Error { error, .. } => JsonValue::String(format!("<error: {}>", error)),
        NuValue::Custom { .. } => JsonValue::String("<custom>".to_string()),
        NuValue::Glob { val, .. } => JsonValue::String(val.to_string()),
        NuValue::CellPath { val, .. } => JsonValue::String(format!("{:?}", val)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_step_engine_creation() {
        let engine = StepEngine::new();
        assert!(engine.error_cache.is_empty());
    }

    #[test]
    fn test_script_hashing() {
        let hash1 = StepEngine::hash_script("echo hello");
        let hash2 = StepEngine::hash_script("echo hello");
        let hash3 = StepEngine::hash_script("echo world");
        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
    }

    // =========================================================================
    // External calls hiding inside closures
    //
    // `validate_ast` walked only top-level pipeline elements, so its
    // ExternalCommandNotAllowed rule never saw anything nested — and nearly all
    // guide logic is nested, inside `where {|e| … }` / `each {|r| … }`. A guide
    // shipped with a bare `and` starting a continuation line inside a closure;
    // nushell has no `and` command, so it parsed as an EXTERNAL CALL, passed
    // validation, passed `nu --ide-check`, and only failed when that closure
    // actually ran — on the last task of the last stage, half an hour in.
    // =========================================================================

    fn is_external_rejection(err: &NuError) -> bool {
        matches!(
            err,
            NuError::Security(SecurityError::ExternalCommandNotAllowed { .. })
        )
    }

    /// Every guide task opens by reading its bindings off `$in`, and that one
    /// line changes the AST completely: nushell rewrites the whole script body
    /// into a single `Expr::Collect` that binds the pipeline input.
    ///
    /// So a validator can look perfectly correct against a bare snippet and
    /// still inspect NOTHING on a real script. These tests carry the prelude the
    /// guides actually use for that reason — the first cut of this fix passed
    /// the prelude-less version of every test below while the real bug sailed
    /// through `dapple guide validate` untouched.
    const IN_PRELUDE: &str = "let inp = $in\nlet b = ($inp.bindings? | default {})\n";

    #[test]
    fn test_rejects_bare_and_continuation_inside_a_closure() {
        // The exact shape from run-the-10k-synthesize.nu:3717, carried on the
        // real `$in` prelude. Line 2 is a complete expression, so line 3 starts
        // a new pipeline element whose first token is `and` — and nushell has no
        // `and` command, so it parses as an external call.
        let engine = StepEngine::new();
        let err = engine
            .validate_script(&format!(
                r#"{IN_PRELUDE}let pd_all = ($b.pd_run? | default [])
let pd_other = ($pd_all | where {{|e|
    (((($e.px_mark? | default "") | into string) | str length) == 0)
        and (((($e.deal_outcome? | default "") | into string) | str length) == 0)
}})"#
            ))
            .expect_err("a bare `and` inside a closure is an external call and must be rejected");
        assert!(is_external_rejection(&err), "got {err:?}");
    }

    #[test]
    fn test_accepts_the_paren_wrapped_multi_line_boolean() {
        // The fix: one paren pair around the whole condition, so the operators
        // stay inside a single expression across the line breaks.
        let engine = StepEngine::new();
        engine
            .validate_script(&format!(
                r#"{IN_PRELUDE}let pd_all = ($b.pd_run? | default [])
let pd_other = ($pd_all | where {{|e|
    ((((($e.px_mark? | default "") | into string) | str length) == 0)
        and (((($e.deal_outcome? | default "") | into string) | str length) == 0))
}})"#
            ))
            .expect("the paren-wrapped form is valid and must keep validating");
    }

    #[test]
    fn test_collect_wrapper_does_not_hide_a_nested_external_call() {
        // The arm that makes the whole guard reachable on real guides.
        let engine = StepEngine::new();
        let err = engine
            .validate_script(&format!(
                "{IN_PRELUDE}[1 2 3] | each {{|x| ^curl https://example.com }}"
            ))
            .expect_err("the $in Collect wrapper must not hide a nested external call");
        assert!(is_external_rejection(&err), "got {err:?}");
    }

    #[test]
    fn test_rejects_an_external_call_nested_in_a_closure() {
        let engine = StepEngine::new();
        let err = engine
            .validate_script("[1 2 3] | each {|x| ^curl https://example.com }")
            .expect_err("an external call one closure deep must be rejected");
        assert!(is_external_rejection(&err), "got {err:?}");
    }

    #[test]
    fn test_rejects_an_external_call_nested_in_a_subexpression() {
        let engine = StepEngine::new();
        let err = engine
            .validate_script("let x = (^whoami); { payload: $x, signals: {} }")
            .expect_err("an external call inside a subexpression must be rejected");
        assert!(is_external_rejection(&err), "got {err:?}");
    }

    #[test]
    fn test_rejects_an_external_call_nested_two_closures_deep() {
        // Depth, not just one level: the walk has to keep descending.
        let engine = StepEngine::new();
        let err = engine
            .validate_script("[[1 2]] | each {|row| $row | each {|c| ^echo $c } }")
            .expect_err("an external call two closures deep must be rejected");
        assert!(is_external_rejection(&err), "got {err:?}");
    }

    #[test]
    fn test_ordinary_nested_guide_script_still_validates() {
        // The guard must not start rejecting the shapes every guide is built
        // from — closures, pipelines, records, string interpolation.
        let engine = StepEngine::new();
        engine
            .validate_script(
                r#"let b = ($in.bindings? | default {})
let rows = ([{n: 1} {n: 2}] | where {|r| $r.n > 1 } | each {|r| { label: $"row ($r.n)", value: $r.n } })
{ payload: { choices: $rows }, signals: { count: ($rows | length) } }"#,
            )
            .expect("ordinary nested guide code must still validate");
    }

    #[test]
    fn test_json_to_nu_conversion() {
        let json = serde_json::json!({
            "name": "test",
            "value": 42,
            "flag": true
        });
        let nu = json_to_nu_value(&json);
        match nu {
            NuValue::Record { .. } => {}
            _ => panic!("Expected record"),
        }
    }

    #[test]
    fn test_error_cache_prevents_reparse() {
        let mut engine = StepEngine::new();
        let bad_script = "this is not valid nushell syntax {{{";
        let input = serde_json::json!({});

        // First call should fail and cache the error
        let result1 = engine.execute_script(bad_script, &input, None);
        assert!(result1.is_err());
        assert!(!engine.error_cache.is_empty());

        // Second call should return the cached error
        let result2 = engine.execute_script(bad_script, &input, None);
        assert!(result2.is_err());

        // Both errors should be the same
        match (result1, result2) {
            (Err(NuError::Parse(msg1)), Err(NuError::Parse(msg2))) => {
                assert_eq!(msg1, msg2);
            }
            _ => panic!("Expected parse errors"),
        }
    }

    #[test]
    fn test_simple_script_execution() {
        let mut engine = StepEngine::new();
        let script = "{ payload: 'hello', signals: {} }";
        let input = serde_json::json!({});

        let result = engine.execute_script(script, &input, None);
        assert!(result.is_ok());

        let output = result.unwrap();
        assert_eq!(output.payload, serde_json::json!("hello"));
    }

    #[test]
    fn test_closure_execution() {
        let mut engine = StepEngine::new();
        // Test that closures work (this requires delta merging)
        let script = r#"
            let items = ["a", "b", "c"];
            let upper = $items | each { |it| $it | str upcase };
            { payload: $upper, signals: {} }
        "#;
        let input = serde_json::json!({});

        let result = engine.execute_script(script, &input, None);
        assert!(
            result.is_ok(),
            "Closure execution should work: {:?}",
            result
        );

        let output = result.unwrap();
        assert_eq!(output.payload, serde_json::json!(["A", "B", "C"]));
    }

    // =========================================================================
    // Failure classification (NuErrorClass)
    //
    // The auto-dispatch retry gate stops re-dispatching a step whose failure is
    // deterministic. Misclassification is asymmetric: deterministic-as-transient
    // burns the bounded retry cap; transient-as-deterministic strands a healthy
    // step. Hence the wildcard default is Transient and these tests pin the
    // variants we explicitly rely on.
    // =========================================================================

    #[test]
    fn missing_column_is_deterministic() {
        let mut engine = StepEngine::new();
        // The shape of a script reading a key its input doesn't carry.
        let script = r#"let x = ({a: 1} | get b); { payload: $x, signals: {} }"#;
        let err = engine
            .execute_script(script, &serde_json::json!({}), None)
            .unwrap_err();
        assert_eq!(err.error_class(), NuErrorClass::Deterministic, "{err:?}");
    }

    #[test]
    fn contract_violation_is_deterministic() {
        let mut engine = StepEngine::new();
        let err = engine
            .execute_script(r#""not a record""#, &serde_json::json!({}), None)
            .unwrap_err();
        assert_eq!(err.error_class(), NuErrorClass::Deterministic, "{err:?}");
    }

    #[test]
    fn parse_error_is_deterministic() {
        let mut engine = StepEngine::new();
        let err = engine
            .execute_script("let x = (", &serde_json::json!({}), None)
            .unwrap_err();
        assert_eq!(err.error_class(), NuErrorClass::Deterministic, "{err:?}");
    }

    #[test]
    fn generic_error_is_transient() {
        // Every custom command (prompt send, edgar, …) raises GenericError — 146
        // construction sites — covering conduit/EDGAR/network failures. Those must
        // keep their retry budget.
        let e = ShellError::GenericError {
            error: "conduit unreachable".into(),
            msg: "connection refused".into(),
            span: None,
            help: None,
            inner: vec![],
        };
        assert_eq!(classify_shell_error(&e), NuErrorClass::Transient);
    }

    #[test]
    fn unsupported_input_is_deterministic() {
        // The 2026-08-02 incident shape: `math sum` over an empty filtered slice.
        let e = ShellError::UnsupportedInput {
            msg: "Unable to give a result with this input".into(),
            input: "value originates from here".into(),
            msg_span: Span::unknown(),
            input_span: Span::unknown(),
        };
        assert_eq!(classify_shell_error(&e), NuErrorClass::Deterministic);
    }

    #[test]
    fn eval_block_wrapper_takes_worst_of_sources() {
        // A failure inside an `each` closure arrives wrapped; classification must
        // reach the sources. Deterministic source ⇒ deterministic whole …
        let det = ShellError::EvalBlockWithInput {
            span: Span::unknown(),
            sources: vec![ShellError::UnsupportedInput {
                msg: "m".into(),
                input: "i".into(),
                msg_span: Span::unknown(),
                input_span: Span::unknown(),
            }],
        };
        assert_eq!(classify_shell_error(&det), NuErrorClass::Deterministic);

        // … while an all-transient wrapper (e.g. `prompt send` failing inside a
        // closure) stays transient.
        let trans = ShellError::EvalBlockWithInput {
            span: Span::unknown(),
            sources: vec![ShellError::GenericError {
                error: "e".into(),
                msg: "m".into(),
                span: None,
                help: None,
                inner: vec![],
            }],
        };
        assert_eq!(classify_shell_error(&trans), NuErrorClass::Transient);
    }
}

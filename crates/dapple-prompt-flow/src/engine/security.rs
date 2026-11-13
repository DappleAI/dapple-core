use nu_protocol::BlockId;
use nu_protocol::ast::{Block, Expr, Expression};
use nu_protocol::engine::StateWorkingSet;
use std::collections::HashSet;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SecurityError {
    #[error("Command '{0}' is not allowed by security policy")]
    DisallowedCommand(String),
    /// Carries the offending command name and its line, because the bare form of
    /// this message is close to useless: guide task scripts run to thousands of
    /// lines, and the usual cause is a typo or a continuation line starting with
    /// an operator (`and` / `or`), which is invisible when you are told only
    /// that *something* somewhere is an external call.
    #[error("External commands are not permitted: `{name}` on line {line}")]
    ExternalCommandNotAllowed { name: String, line: usize },
    #[error("Environment mutation is not permitted")]
    EnvMutationNotAllowed,
    #[error("Plugin commands are not permitted")]
    PluginNotAllowed,
}

#[derive(Debug, Clone)]
pub enum SecurityMode {
    /// Allow all internal Nushell commands. External commands are still blocked.
    Permissive,
    /// Only allow commands in the specified allowlist.
    Allowlist(HashSet<String>),
}

#[derive(Debug, Clone)]
pub struct SecurityPolicy {
    mode: SecurityMode,
    max_metadata_size: usize,
}

impl Default for SecurityPolicy {
    fn default() -> Self {
        Self::new_with_defaults()
    }
}

impl SecurityPolicy {
    /// Create a new policy with allowlist mode
    pub fn new_with_allowlist(allowed_commands: HashSet<String>, max_metadata_size: usize) -> Self {
        Self {
            mode: SecurityMode::Allowlist(allowed_commands),
            max_metadata_size,
        }
    }

    /// Create a new permissive policy (allows all internal commands)
    pub fn new_permissive() -> Self {
        Self {
            mode: SecurityMode::Permissive,
            max_metadata_size: 512 * 1024, // 512KB default
        }
    }

    /// Create default policy - uses permissive mode
    pub fn new_with_defaults() -> Self {
        Self::new_permissive()
    }

    /// Create an allowlist-based policy with the standard set of safe commands
    pub fn new_with_standard_allowlist() -> Self {
        let mut allowed = HashSet::new();

        // Core language constructs
        allowed.insert("if".to_string());
        allowed.insert("else".to_string());
        allowed.insert("match".to_string());
        allowed.insert("for".to_string());
        allowed.insert("while".to_string());
        allowed.insert("loop".to_string());
        allowed.insert("break".to_string());
        allowed.insert("continue".to_string());
        allowed.insert("return".to_string());
        allowed.insert("try".to_string());
        allowed.insert("catch".to_string());
        allowed.insert("def".to_string());
        allowed.insert("let".to_string());
        allowed.insert("mut".to_string());
        allowed.insert("const".to_string());

        // Data manipulation
        allowed.insert("echo".to_string());
        allowed.insert("print".to_string());
        allowed.insert("get".to_string());
        allowed.insert("select".to_string());
        allowed.insert("reject".to_string());
        allowed.insert("where".to_string());
        allowed.insert("filter".to_string());
        allowed.insert("each".to_string());
        allowed.insert("par-each".to_string());
        allowed.insert("reduce".to_string());
        allowed.insert("fold".to_string());
        allowed.insert("flatten".to_string());
        allowed.insert("transpose".to_string());
        allowed.insert("group-by".to_string());
        allowed.insert("sort-by".to_string());
        allowed.insert("uniq".to_string());
        allowed.insert("uniq-by".to_string());
        allowed.insert("first".to_string());
        allowed.insert("last".to_string());
        allowed.insert("take".to_string());
        allowed.insert("skip".to_string());
        allowed.insert("drop".to_string());
        allowed.insert("reverse".to_string());
        allowed.insert("length".to_string());
        allowed.insert("range".to_string());
        allowed.insert("enumerate".to_string());
        allowed.insert("zip".to_string());
        allowed.insert("merge".to_string());
        allowed.insert("insert".to_string());
        allowed.insert("update".to_string());
        allowed.insert("upsert".to_string());
        allowed.insert("append".to_string());
        allowed.insert("prepend".to_string());
        allowed.insert("wrap".to_string());
        allowed.insert("unwrap".to_string());
        allowed.insert("default".to_string());
        allowed.insert("compact".to_string());

        // String operations
        allowed.insert("str".to_string());
        allowed.insert("str join".to_string());
        allowed.insert("str trim".to_string());
        allowed.insert("str length".to_string());
        allowed.insert("str replace".to_string());
        allowed.insert("str substring".to_string());
        allowed.insert("str contains".to_string());
        allowed.insert("str starts-with".to_string());
        allowed.insert("str ends-with".to_string());
        allowed.insert("str index-of".to_string());
        allowed.insert("str upcase".to_string());
        allowed.insert("str downcase".to_string());
        allowed.insert("str capitalize".to_string());
        allowed.insert("split".to_string());
        allowed.insert("split row".to_string());
        allowed.insert("split column".to_string());
        allowed.insert("lines".to_string());
        allowed.insert("parse".to_string());
        allowed.insert("format".to_string());
        allowed.insert("fill".to_string());

        // Type conversions
        allowed.insert("into".to_string());
        allowed.insert("into string".to_string());
        allowed.insert("into int".to_string());
        allowed.insert("into float".to_string());
        allowed.insert("into bool".to_string());
        allowed.insert("into record".to_string());
        allowed.insert("into binary".to_string());

        // JSON operations
        allowed.insert("from json".to_string());
        allowed.insert("to json".to_string());
        allowed.insert("from csv".to_string());
        allowed.insert("to csv".to_string());
        allowed.insert("from yaml".to_string());
        allowed.insert("to yaml".to_string());
        allowed.insert("from toml".to_string());
        allowed.insert("to toml".to_string());

        // Math operations
        allowed.insert("math".to_string());
        allowed.insert("math abs".to_string());
        allowed.insert("math ceil".to_string());
        allowed.insert("math floor".to_string());
        allowed.insert("math round".to_string());
        allowed.insert("math sqrt".to_string());
        allowed.insert("math sum".to_string());
        allowed.insert("math avg".to_string());
        allowed.insert("math min".to_string());
        allowed.insert("math max".to_string());

        // Comparison/logic
        allowed.insert("is-empty".to_string());
        allowed.insert("is-not-empty".to_string());
        allowed.insert("all".to_string());
        allowed.insert("any".to_string());
        allowed.insert("not".to_string());

        // Date/time (non-system)
        allowed.insert("date now".to_string());
        allowed.insert("date format".to_string());
        allowed.insert("date to-record".to_string());

        // Utility
        allowed.insert("describe".to_string());
        allowed.insert("metadata".to_string());
        allowed.insert("debug".to_string());
        allowed.insert("error make".to_string());
        allowed.insert("null".to_string());
        allowed.insert("ignore".to_string());
        allowed.insert("collect".to_string());

        // Record/table creation
        allowed.insert("headers".to_string());
        allowed.insert("values".to_string());
        allowed.insert("columns".to_string());
        allowed.insert("items".to_string());

        // Dapple prompt commands
        allowed.insert("prompt".to_string());
        allowed.insert("prompt send".to_string());
        allowed.insert("prompt retry".to_string());
        allowed.insert("prompt reset".to_string());

        // Value-slot binding commands (envelope-delivery handle resolution)
        allowed.insert("binding".to_string());
        allowed.insert("binding open".to_string());

        // Dapple guide commands
        allowed.insert("guide".to_string());
        allowed.insert("guide search".to_string());

        // Dapple asset commands
        allowed.insert("asset".to_string());
        allowed.insert("asset text".to_string());
        allowed.insert("asset status".to_string());

        // Dapple image commands: `imagine` (generate pieces) + `composite`
        // (low-level, caller-positioned assembly) + `imager compose`
        // (strategy-driven, content-aware assembly). No generative edit/inpaint.
        allowed.insert("imagine".to_string());
        allowed.insert("composite".to_string());
        allowed.insert("imager".to_string());
        allowed.insert("imager compose".to_string());

        // Dapple video command: `animate` generates net-new clips (the video
        // analog of `imagine`). No generative edit path; assembly is `av *`.
        allowed.insert("animate".to_string());

        // Dapple A/V commands: `av compose` arranges generated audio/video pieces
        // into an editable OTIO timeline document (no model re-encodes the media).
        allowed.insert("av".to_string());
        allowed.insert("av compose".to_string());
        allowed.insert("av mix".to_string());
        allowed.insert("av render".to_string());
        allowed.insert("av snapshot".to_string());

        // Dapple audio-analysis commands: `audio analyze` turns multitrack WAV
        // stems into an annotated song map via the conduit-oximedia media service.
        allowed.insert("audio".to_string());
        allowed.insert("audio analyze".to_string());

        // Dapple SVG mechanics: `svg shapes` extracts basic shape geometry
        // (pure local parse) so guide tasks can compile composition documents.
        allowed.insert("svg".to_string());
        allowed.insert("svg shapes".to_string());

        // Dapple media-gateway commands: Twitch VOD → learning-highlight clips via
        // the opt-in conduit-media-gateway service (DAPPLE_MEDIA_GATEWAY_URL).
        allowed.insert("twitch".to_string());
        allowed.insert("twitch pull".to_string());
        allowed.insert("vod".to_string());
        allowed.insert("vod analyze".to_string());
        allowed.insert("vod transcribe".to_string());
        allowed.insert("vod detect".to_string());
        allowed.insert("vod context".to_string());
        allowed.insert("vod cut".to_string());
        allowed.insert("vod assemble".to_string());
        allowed.insert("vod job".to_string());

        // Dapple SEC EDGAR commands: filings sectioned by Item, XBRL facts, and
        // fund identity/holdings (opt-in at call time via DAPPLE_EDGAR_USER_AGENT).
        // Keep this list in sync with the registrations in `commands::register` —
        // a subcommand missing here is invisible in allowlist mode.
        allowed.insert("edgar".to_string());
        allowed.insert("edgar filings".to_string());
        allowed.insert("edgar facts".to_string());
        allowed.insert("edgar exhibits".to_string());
        allowed.insert("edgar holders".to_string());
        allowed.insert("edgar insiders".to_string());
        allowed.insert("edgar segments".to_string());
        allowed.insert("edgar manager".to_string());
        allowed.insert("edgar security".to_string());
        allowed.insert("edgar fund".to_string());
        allowed.insert("edgar nport".to_string());
        allowed.insert("edgar ncen".to_string());
        allowed.insert("edgar nmfp".to_string());
        allowed.insert("edgar nmfp-universe".to_string());
        allowed.insert("edgar fund-sections".to_string());
        allowed.insert("edgar resolve".to_string());
        allowed.insert("edgar company-cik".to_string());
        allowed.insert("edgar search".to_string());
        allowed.insert("edgar adv".to_string());
        allowed.insert("edgar portfolio".to_string());
        allowed.insert("edgar bdc".to_string());
        allowed.insert("edgar book".to_string());

        // Dapple document commands: markdown → PDF document asset (deterministic
        // conduit render; the report-deliverable path).
        allowed.insert("document".to_string());
        allowed.insert("document render".to_string());

        // Dapple fluent commands (deterministic Fluent-file emission)
        allowed.insert("fluent".to_string());
        allowed.insert("fluent emit-guide".to_string());

        // Shared report furniture: markdown emission and number display. Pure
        // string building, no I/O of any kind.
        allowed.insert("md".to_string());
        allowed.insert("md table".to_string());
        allowed.insert("md admonish".to_string());
        allowed.insert("fmt".to_string());
        allowed.insert("fmt scale".to_string());
        allowed.insert("fmt usd".to_string());
        allowed.insert("fmt round".to_string());

        // Long-text sampling (pure local string work, no I/O).
        allowed.insert("text".to_string());
        allowed.insert("text window".to_string());

        Self {
            mode: SecurityMode::Allowlist(allowed),
            max_metadata_size: 512 * 1024, // 512KB default
        }
    }

    pub fn max_metadata_size(&self) -> usize {
        self.max_metadata_size
    }

    pub fn is_command_allowed(&self, name: &str) -> bool {
        match &self.mode {
            SecurityMode::Permissive => true,
            SecurityMode::Allowlist(allowed) => allowed.contains(name),
        }
    }

    /// Walk a parsed script's AST and reject anything the policy forbids.
    ///
    /// `working_set` is required because a closure / subexpression / block is
    /// stored in the AST as a `BlockId`, not inline — without it those bodies
    /// cannot be reached at all. That is not a theoretical gap: essentially all
    /// guide logic lives inside closures (`where {|e| … }`, `each {|r| … }`), so
    /// while this walked only top-level pipeline elements the
    /// [`SecurityError::ExternalCommandNotAllowed`] arm below was effectively
    /// dead code, and an external call nested one closure deep passed validation
    /// and blew up at RUNTIME instead — on whatever stage happened to execute it.
    pub fn validate_ast(
        &self,
        block: &Block,
        working_set: &StateWorkingSet,
    ) -> Result<(), SecurityError> {
        let mut visited = HashSet::new();
        self.validate_block(block, working_set, &mut visited)
    }

    fn validate_block(
        &self,
        block: &Block,
        working_set: &StateWorkingSet,
        visited: &mut HashSet<BlockId>,
    ) -> Result<(), SecurityError> {
        for pipeline in &block.pipelines {
            for element in &pipeline.elements {
                // In nu-protocol 0.100, PipelineElement has expr field directly
                self.validate_expression(&element.expr, working_set, visited)?;
            }
        }
        Ok(())
    }

    /// Resolve a referenced block and walk it, guarding against revisiting one.
    ///
    /// The `visited` set is not just belt-and-braces: a captured closure can be
    /// referenced from more than one place in a script, so without it a walk can
    /// recurse without end on a self-referential capture.
    fn validate_block_id(
        &self,
        block_id: BlockId,
        working_set: &StateWorkingSet,
        visited: &mut HashSet<BlockId>,
    ) -> Result<(), SecurityError> {
        if !visited.insert(block_id) {
            return Ok(());
        }
        let block = working_set.get_block(block_id);
        self.validate_block(block, working_set, visited)
    }

    fn validate_expression(
        &self,
        expr: &Expression,
        working_set: &StateWorkingSet,
        visited: &mut HashSet<BlockId>,
    ) -> Result<(), SecurityError> {
        match &expr.expr {
            Expr::Call(call) => {
                // For calls, we validate arguments recursively
                // Command validation happens at parse time with the full working set
                for arg in &call.arguments {
                    if let Some(e) = arg.expr() {
                        self.validate_expression(e, working_set, visited)?;
                    }
                }
                Ok(())
            }
            Expr::ExternalCall(head, _) => {
                // Name and line come from the source the working set is holding,
                // so the report points at the offending token instead of the
                // whole script.
                let bytes = working_set.get_span_contents(head.span);
                let name = String::from_utf8_lossy(bytes).trim().to_string();
                let line = working_set
                    .get_span_contents(nu_protocol::Span::new(0, head.span.start))
                    .iter()
                    .filter(|b| **b == b'\n')
                    .count()
                    + 1;
                Err(SecurityError::ExternalCommandNotAllowed { name, line })
            }
            // Every block-bearing variant is resolved through the working set and
            // walked. These four used to return Ok without descending, which is
            // what let a bare `and` at the start of a line inside a closure body
            // (parsed as an external call to a command named `and`) reach
            // production.
            Expr::Subexpression(block_id) => {
                self.validate_block_id(*block_id, working_set, visited)
            }
            Expr::Block(block_id) => self.validate_block_id(*block_id, working_set, visited),
            Expr::Closure(block_id) => self.validate_block_id(*block_id, working_set, visited),
            // `Expr::Collect` is the single most important arm here, because it
            // wraps EVERY real guide task. When a script mentions `$in` — and the
            // house pattern opens with `let b = ($in.bindings? | default {})` —
            // nushell rewrites the whole script body into one `Collect`
            // expression that binds the pipeline input. Without this arm the
            // entire script hides behind it and the walk inspects nothing at all,
            // which is precisely how a bare `and` reached production and how the
            // first cut of this very fix still passed every guide.
            Expr::Collect(_, inner) => self.validate_expression(inner, working_set, visited),
            Expr::FullCellPath(fcp) => self.validate_expression(&fcp.head, working_set, visited),
            Expr::BinaryOp(lhs, _, rhs) => {
                self.validate_expression(lhs, working_set, visited)?;
                self.validate_expression(rhs, working_set, visited)
            }
            Expr::UnaryNot(e) => self.validate_expression(e, working_set, visited),
            Expr::List(items) => {
                for item in items {
                    self.validate_expression(item.expr(), working_set, visited)?;
                }
                Ok(())
            }
            Expr::Record(fields) => {
                for field in fields {
                    match field {
                        nu_protocol::ast::RecordItem::Pair(k, v) => {
                            self.validate_expression(k, working_set, visited)?;
                            self.validate_expression(v, working_set, visited)?;
                        }
                        nu_protocol::ast::RecordItem::Spread(_, e) => {
                            self.validate_expression(e, working_set, visited)?;
                        }
                    }
                }
                Ok(())
            }
            Expr::Table(table) => {
                for col in &table.columns {
                    self.validate_expression(col, working_set, visited)?;
                }
                for row in &table.rows {
                    for cell in row {
                        self.validate_expression(cell, working_set, visited)?;
                    }
                }
                Ok(())
            }
            Expr::RowCondition(block_id) => self.validate_block_id(*block_id, working_set, visited),
            Expr::StringInterpolation(parts) => {
                for p in parts {
                    self.validate_expression(p, working_set, visited)?;
                }
                Ok(())
            }
            Expr::GlobInterpolation(parts, _) => {
                for p in parts {
                    self.validate_expression(p, working_set, visited)?;
                }
                Ok(())
            }
            Expr::MatchBlock(cases) => {
                for (_pattern, expr) in cases {
                    self.validate_expression(expr, working_set, visited)?;
                }
                Ok(())
            }
            // All other expression types are considered safe
            // (literals, variables, operators, etc.)
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_policy_is_permissive() {
        let policy = SecurityPolicy::default();
        // Permissive mode allows all commands
        assert!(policy.is_command_allowed("echo"));
        assert!(policy.is_command_allowed("any-command-at-all"));
        assert!(policy.is_command_allowed("made-up-command"));
    }

    #[test]
    fn test_permissive_policy_allows_everything() {
        let policy = SecurityPolicy::new_permissive();
        assert!(policy.is_command_allowed("echo"));
        assert!(policy.is_command_allowed("str"));
        assert!(policy.is_command_allowed("str trim"));
        assert!(policy.is_command_allowed("random-command"));
    }

    #[test]
    fn test_standard_allowlist_policy() {
        let policy = SecurityPolicy::new_with_standard_allowlist();
        assert!(policy.is_command_allowed("echo"));
        assert!(policy.is_command_allowed("str"));
        assert!(policy.is_command_allowed("str trim"));
        // Commands not in the allowlist should be blocked
        assert!(!policy.is_command_allowed("random-command"));
        assert!(!policy.is_command_allowed("exec"));
    }

    #[test]
    fn test_custom_allowlist_policy() {
        let mut allowed = HashSet::new();
        allowed.insert("echo".to_string());
        let policy = SecurityPolicy::new_with_allowlist(allowed, 1024);
        assert!(policy.is_command_allowed("echo"));
        assert!(!policy.is_command_allowed("str"));
    }
}

# Command Authoring

> **Audience:** Backend Developers · Platform Engineers

Guide for implementing new custom Nushell commands in dapple-prompt-flow.

## Command Architecture

Custom commands are implemented in Rust and registered with the Nushell engine:

```rust
pub struct MyCommand;

impl Command for MyCommand {
    fn name(&self) -> &str {
        "my-command"
    }

    fn signature(&self) -> Signature {
        Signature::build("my-command")
            .input_output_types(vec![(Type::String, Type::String)])
            .required("arg", SyntaxShape::String, "Required argument")
            .named("flag", SyntaxShape::Int, "Optional flag", Some('f'))
            .category(Category::Custom("dapple".into()))
    }

    fn usage(&self) -> &str {
        "Description of the command"
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        // Implementation
    }
}
```

## Registration

Commands are registered when creating the StepEngine:

```rust
// src/engine/mod.rs
fn register_commands(engine_state: &mut EngineState) {
    // Built-in dapple commands
    bind_command!(
        engine_state,
        MadlibCommand,
        SummarizeCommand,
        JsonSchemaCommand,
        PromptCommand
    );
}
```

## Input/Output Types

Specify allowed input and output types:

```rust
fn signature(&self) -> Signature {
    Signature::build("transform")
        .input_output_types(vec![
            (Type::String, Type::String),
            (Type::List(Box::new(Type::String)), Type::String),
            (Type::Record(vec![]), Type::Record(vec![])),
        ])
}
```

## Accessing Input

```rust
fn run(&self, ..., input: PipelineData) -> Result<PipelineData, ShellError> {
    // Get input as string
    let input_str = input.into_value(call.head)?.as_string()?;

    // Or handle multiple types
    match input.into_value(call.head)? {
        Value::String { val, .. } => process_string(val),
        Value::List { vals, .. } => process_list(vals),
        Value::Record { val, .. } => process_record(val),
        _ => Err(ShellError::TypeMismatch { ... }),
    }
}
```

## Returning Values

```rust
// Return string
Ok(PipelineData::Value(
    Value::string(result, call.head),
    None,
))

// Return record
let record = Record::from_iter([
    ("field1".into(), Value::string("value1", call.head)),
    ("field2".into(), Value::int(42, call.head)),
]);
Ok(PipelineData::Value(
    Value::record(record, call.head),
    None,
))
```

## Async Operations

For commands that need async (e.g., HTTP calls):

```rust
fn run(&self, ...) -> Result<PipelineData, ShellError> {
    // Create runtime or use existing
    let runtime = tokio::runtime::Runtime::new()
        .map_err(|e| ShellError::GenericError { ... })?;

    let result = runtime.block_on(async {
        // Async operation
        let response = client.post(url).send().await?;
        response.text().await
    })?;

    Ok(PipelineData::Value(Value::string(result, call.head), None))
}
```

## Error Handling

Return descriptive errors:

```rust
Err(ShellError::GenericError {
    error: "Madlib failed".into(),
    msg: "Check that conduit is running and MLM is enabled".into(),
    span: Some(call.head),
    help: Some("Run: MLM_ENABLED=true cargo run -p dapple-prompt-conduit".into()),
    inner: vec![],
})
```

## Testing Commands

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use nu_protocol::engine::EngineState;

    #[test]
    fn test_my_command() {
        let engine_state = create_test_engine();
        let mut stack = Stack::new();
        let call = create_test_call(&["my-command"]);

        let input = PipelineData::Value(
            Value::string("test input", Span::test_data()),
            None,
        );

        let result = MyCommand.run(&engine_state, &mut stack, &call, input);
        assert!(result.is_ok());
    }
}
```

## Best Practices

1. **Type safety** - Use specific input/output types, not `Type::Any`
2. **Error messages** - Include actionable help text
3. **Documentation** - Write clear usage and examples
4. **Testing** - Test all input type combinations
5. **Performance** - Avoid blocking in hot paths
6. **Security** - Validate inputs, don't execute arbitrary code

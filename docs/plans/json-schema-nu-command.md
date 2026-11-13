# json-schema Nushell Command

## Objective

Add a `json-schema` Nushell command that infers a JSON Schema from input JSON data. Useful for generating schemas from example payloads.

## Command Signature

```nushell
# Infer schema from JSON
{ name: "Alice", age: 30 } | json-schema
# → { "type": "object", "properties": { "name": { "type": "string" }, "age": { "type": "integer" } } }

# From file
open example.json | json-schema

# With title
{ id: 1 } | json-schema --title "MySchema"
```

## Implementation

```rust
// crates/dapple-prompt-flow/src/commands/json_schema.rs

use nu_engine::command_prelude::*;
use serde_json::Value as JsonValue;

#[derive(Clone)]
pub struct JsonSchemaCommand;

impl Command for JsonSchemaCommand {
    fn name(&self) -> &str {
        "json-schema"
    }

    fn signature(&self) -> Signature {
        Signature::build("json-schema")
            .input_output_types(vec![
                (Type::Any, Type::Record(vec![])),
            ])
            .named("title", SyntaxShape::String, "Schema title", Some('t'))
            .switch("required-all", "Mark all properties as required", Some('r'))
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Infer a JSON Schema from input JSON data."
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let span = call.head;
        let title: Option<String> = call.get_flag(engine_state, stack, "title")?;
        let required_all: bool = call.has_flag(engine_state, stack, "required-all")?;

        let input_value = input.into_value(span)?;
        let json = nu_value_to_json(&input_value)?;

        let schema = infer_schema(&json, title, required_all);
        let output = json_to_nu_value(&schema, span);

        Ok(PipelineData::Value(output, None))
    }
}

fn infer_schema(value: &JsonValue, title: Option<String>, required_all: bool) -> JsonValue {
    let mut schema = match value {
        JsonValue::Null => serde_json::json!({ "type": "null" }),
        JsonValue::Bool(_) => serde_json::json!({ "type": "boolean" }),
        JsonValue::Number(n) => {
            if n.is_i64() || n.is_u64() {
                serde_json::json!({ "type": "integer" })
            } else {
                serde_json::json!({ "type": "number" })
            }
        }
        JsonValue::String(_) => serde_json::json!({ "type": "string" }),
        JsonValue::Array(arr) => {
            if arr.is_empty() {
                serde_json::json!({ "type": "array", "items": {} })
            } else {
                // Infer from first element (could be smarter with union types)
                let items_schema = infer_schema(&arr[0], None, required_all);
                serde_json::json!({ "type": "array", "items": items_schema })
            }
        }
        JsonValue::Object(obj) => {
            let mut properties = serde_json::Map::new();
            let mut required = Vec::new();

            for (key, val) in obj {
                properties.insert(key.clone(), infer_schema(val, None, required_all));
                if required_all {
                    required.push(JsonValue::String(key.clone()));
                }
            }

            let mut schema = serde_json::json!({
                "type": "object",
                "properties": properties
            });

            if required_all && !required.is_empty() {
                schema["required"] = JsonValue::Array(required);
            }

            schema
        }
    };

    if let Some(t) = title {
        schema["title"] = JsonValue::String(t);
    }

    schema
}
```

## Register Command

```rust
// crates/dapple-prompt-flow/src/commands/mod.rs

pub mod json_schema;

// In register_dapple_commands:
working_set.add_decl(Box::new(json_schema::JsonSchemaCommand));
```

## Files to Create/Modify

| File | Action | Description |
|------|--------|-------------|
| `src/commands/json_schema.rs` | Create | Command implementation |
| `src/commands/mod.rs` | Modify | Register command |

## Examples

```nushell
# Simple object
{ name: "test", count: 5 } | json-schema
# → { type: "object", properties: { name: { type: "string" }, count: { type: "integer" } } }

# With required fields
{ id: 1, name: "x" } | json-schema --required-all
# → { type: "object", properties: {...}, required: ["id", "name"] }

# Array
[1, 2, 3] | json-schema
# → { type: "array", items: { type: "integer" } }

# Nested
{ user: { name: "A", tags: ["x"] } } | json-schema
# → nested schema structure
```

## Acceptance Criteria

- [ ] `json-schema` command registered
- [ ] Infers types: null, boolean, integer, number, string, array, object
- [ ] Handles nested structures
- [ ] `--title` adds schema title
- [ ] `--required-all` marks all properties required
- [ ] Pure flow, no external dependencies beyond serde_json

## Future Enhancements

- Union types for heterogeneous arrays
- `--draft` flag for JSON Schema draft version
- Format detection (email, uri, date-time, uuid)
- `$schema` and `$id` fields

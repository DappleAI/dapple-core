//! The `json-schema` command for inferring JSON Schema from data

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
            .input_output_types(vec![(Type::Any, Type::record())])
            .named("title", SyntaxShape::String, "Schema title", Some('t'))
            .switch("required-all", "Mark all properties as required", Some('r'))
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Infer a JSON Schema from input data."
    }

    fn extra_description(&self) -> &str {
        r#"Takes any Nushell value via pipeline and returns a JSON Schema
that describes its structure.

The inferred schema follows JSON Schema draft-07 conventions:
- null → { "type": "null" }
- bool → { "type": "boolean" }
- integers → { "type": "integer" }
- floats → { "type": "number" }
- strings → { "type": "string" }
- arrays → { "type": "array", "items": <inferred from first element> }
- records → { "type": "object", "properties": {...} }

Flags:
- --title/-t: Adds a "title" field to the root schema
- --required-all/-r: Marks all object properties as required
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["json-schema", "schema", "infer", "type", "validate"]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![
            Example {
                description: "Infer schema from a simple record",
                example: r#"{ name: "test", age: 30 } | json-schema"#,
                result: None,
            },
            Example {
                description: "Infer schema with a title",
                example: r#"{ id: 1, active: true } | json-schema --title "User""#,
                result: None,
            },
            Example {
                description: "Infer schema with all properties required",
                example: r#"{ x: 1, y: 2 } | json-schema --required-all"#,
                result: None,
            },
            Example {
                description: "Infer schema from an array",
                example: r#"[1, 2, 3] | json-schema"#,
                result: None,
            },
        ]
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let span = call.head;

        // Get flags
        let title: Option<String> = call.get_flag(engine_state, stack, "title")?;
        let required_all = call.has_flag(engine_state, stack, "required-all")?;

        // Convert input to value
        let input_value = input.into_value(span)?;

        // Convert to JSON for schema inference
        let json_value = value_to_json(&input_value, span)?;

        // Infer schema
        let schema = infer_schema(&json_value, title, required_all);

        // Convert back to Nushell value
        let result = json_to_nu_value(&schema, span)?;

        Ok(PipelineData::Value(result, None))
    }
}

/// Convert a Nushell Value to serde_json::Value
#[allow(clippy::result_large_err, clippy::only_used_in_recursion)]
fn value_to_json(value: &Value, span: Span) -> Result<JsonValue, ShellError> {
    match value {
        Value::Nothing { .. } => Ok(JsonValue::Null),
        Value::Bool { val, .. } => Ok(JsonValue::Bool(*val)),
        Value::Int { val, .. } => Ok(JsonValue::Number((*val).into())),
        Value::Float { val, .. } => {
            if let Some(n) = serde_json::Number::from_f64(*val) {
                Ok(JsonValue::Number(n))
            } else {
                // Handle NaN/Infinity by converting to null
                Ok(JsonValue::Null)
            }
        }
        Value::String { val, .. } => Ok(JsonValue::String(val.clone())),
        Value::List { vals, .. } => {
            let arr: Result<Vec<JsonValue>, _> =
                vals.iter().map(|v| value_to_json(v, span)).collect();
            Ok(JsonValue::Array(arr?))
        }
        Value::Record { val, .. } => {
            let mut map = serde_json::Map::new();
            for (k, v) in val.iter() {
                map.insert(k.clone(), value_to_json(v, span)?);
            }
            Ok(JsonValue::Object(map))
        }
        Value::Date { val, .. } => Ok(JsonValue::String(val.to_string())),
        Value::Duration { val, .. } => Ok(JsonValue::Number((*val).into())),
        Value::Filesize { val, .. } => Ok(JsonValue::Number(val.get().into())),
        Value::Binary { val, .. } => {
            // Encode binary as base64 string
            use base64::Engine;
            let encoded = base64::engine::general_purpose::STANDARD.encode(val);
            Ok(JsonValue::String(encoded))
        }
        _ => {
            // Fallback: convert to string representation
            Ok(JsonValue::String(
                value.to_expanded_string("", &nu_protocol::Config::default()),
            ))
        }
    }
}

/// Convert a serde_json::Value to Nushell Value
#[allow(clippy::result_large_err)]
fn json_to_nu_value(json: &JsonValue, span: Span) -> Result<Value, ShellError> {
    match json {
        JsonValue::Null => Ok(Value::nothing(span)),
        JsonValue::Bool(b) => Ok(Value::bool(*b, span)),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(Value::int(i, span))
            } else if let Some(f) = n.as_f64() {
                Ok(Value::float(f, span))
            } else {
                // Fallback for very large numbers
                Ok(Value::string(n.to_string(), span))
            }
        }
        JsonValue::String(s) => Ok(Value::string(s.clone(), span)),
        JsonValue::Array(arr) => {
            let vals: Result<Vec<Value>, _> =
                arr.iter().map(|v| json_to_nu_value(v, span)).collect();
            Ok(Value::list(vals?, span))
        }
        JsonValue::Object(obj) => {
            let mut record = Record::new();
            for (k, v) in obj.iter() {
                record.push(k.clone(), json_to_nu_value(v, span)?);
            }
            Ok(Value::record(record, span))
        }
    }
}

/// Infer a JSON Schema from a JSON value
fn infer_schema(value: &JsonValue, title: Option<String>, required_all: bool) -> JsonValue {
    let mut schema = infer_schema_inner(value, required_all);

    // Add title at root level if provided
    if let (Some(t), JsonValue::Object(map)) = (title, &mut schema) {
        map.insert("title".to_string(), JsonValue::String(t));
    }

    schema
}

/// Inner recursive schema inference
fn infer_schema_inner(value: &JsonValue, required_all: bool) -> JsonValue {
    match value {
        JsonValue::Null => {
            serde_json::json!({ "type": "null" })
        }
        JsonValue::Bool(_) => {
            serde_json::json!({ "type": "boolean" })
        }
        JsonValue::Number(n) => {
            if n.is_i64() || n.is_u64() {
                serde_json::json!({ "type": "integer" })
            } else {
                serde_json::json!({ "type": "number" })
            }
        }
        JsonValue::String(_) => {
            serde_json::json!({ "type": "string" })
        }
        JsonValue::Array(arr) => {
            if arr.is_empty() {
                serde_json::json!({
                    "type": "array",
                    "items": {}
                })
            } else {
                // Infer items schema from first element
                let items_schema = infer_schema_inner(&arr[0], required_all);
                serde_json::json!({
                    "type": "array",
                    "items": items_schema
                })
            }
        }
        JsonValue::Object(obj) => {
            let mut properties = serde_json::Map::new();
            let mut required_props: Vec<String> = Vec::new();

            for (k, v) in obj.iter() {
                properties.insert(k.clone(), infer_schema_inner(v, required_all));
                if required_all {
                    required_props.push(k.clone());
                }
            }

            let mut schema_obj = serde_json::Map::new();
            schema_obj.insert("type".to_string(), JsonValue::String("object".to_string()));
            schema_obj.insert("properties".to_string(), JsonValue::Object(properties));

            if required_all && !required_props.is_empty() {
                // Sort for deterministic output
                required_props.sort();
                schema_obj.insert(
                    "required".to_string(),
                    JsonValue::Array(required_props.into_iter().map(JsonValue::String).collect()),
                );
            }

            JsonValue::Object(schema_obj)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_name() {
        let cmd = JsonSchemaCommand;
        assert_eq!(cmd.name(), "json-schema");
    }

    #[test]
    fn test_command_description() {
        let cmd = JsonSchemaCommand;
        assert!(!cmd.description().is_empty());
        assert!(cmd.description().contains("JSON Schema"));
    }

    #[test]
    fn test_infer_null() {
        let schema = infer_schema(&JsonValue::Null, None, false);
        assert_eq!(schema, serde_json::json!({ "type": "null" }));
    }

    #[test]
    fn test_infer_boolean() {
        let schema = infer_schema(&JsonValue::Bool(true), None, false);
        assert_eq!(schema, serde_json::json!({ "type": "boolean" }));
    }

    #[test]
    fn test_infer_integer() {
        let schema = infer_schema(&serde_json::json!(42), None, false);
        assert_eq!(schema, serde_json::json!({ "type": "integer" }));
    }

    #[test]
    fn test_infer_number() {
        let schema = infer_schema(&serde_json::json!(3.5), None, false);
        assert_eq!(schema, serde_json::json!({ "type": "number" }));
    }

    #[test]
    fn test_infer_string() {
        let schema = infer_schema(&serde_json::json!("hello"), None, false);
        assert_eq!(schema, serde_json::json!({ "type": "string" }));
    }

    #[test]
    fn test_infer_empty_array() {
        let schema = infer_schema(&serde_json::json!([]), None, false);
        assert_eq!(
            schema,
            serde_json::json!({
                "type": "array",
                "items": {}
            })
        );
    }

    #[test]
    fn test_infer_array_of_integers() {
        let schema = infer_schema(&serde_json::json!([1, 2, 3]), None, false);
        assert_eq!(
            schema,
            serde_json::json!({
                "type": "array",
                "items": { "type": "integer" }
            })
        );
    }

    #[test]
    fn test_infer_object() {
        let schema = infer_schema(
            &serde_json::json!({
                "name": "test",
                "age": 30
            }),
            None,
            false,
        );

        let expected = serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "age": { "type": "integer" }
            }
        });

        assert_eq!(schema, expected);
    }

    #[test]
    fn test_infer_object_with_required() {
        let schema = infer_schema(
            &serde_json::json!({
                "x": 1,
                "y": 2
            }),
            None,
            true,
        );

        // Check type
        assert_eq!(schema["type"], "object");
        // Check properties exist
        assert!(schema["properties"]["x"]["type"] == "integer");
        assert!(schema["properties"]["y"]["type"] == "integer");
        // Check required array (sorted)
        let required = schema["required"].as_array().unwrap();
        assert_eq!(required.len(), 2);
        assert!(required.contains(&JsonValue::String("x".to_string())));
        assert!(required.contains(&JsonValue::String("y".to_string())));
    }

    #[test]
    fn test_infer_with_title() {
        let schema = infer_schema(
            &serde_json::json!({ "id": 1 }),
            Some("TestSchema".to_string()),
            false,
        );

        assert_eq!(schema["title"], "TestSchema");
        assert_eq!(schema["type"], "object");
    }

    #[test]
    fn test_infer_nested_object() {
        let schema = infer_schema(
            &serde_json::json!({
                "user": {
                    "name": "Alice",
                    "active": true
                }
            }),
            None,
            false,
        );

        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["user"]["type"], "object");
        assert_eq!(
            schema["properties"]["user"]["properties"]["name"]["type"],
            "string"
        );
        assert_eq!(
            schema["properties"]["user"]["properties"]["active"]["type"],
            "boolean"
        );
    }

    #[test]
    fn test_value_to_json_primitives() {
        let span = Span::test_data();

        // null
        let result = value_to_json(&Value::nothing(span), span).unwrap();
        assert_eq!(result, JsonValue::Null);

        // bool
        let result = value_to_json(&Value::bool(true, span), span).unwrap();
        assert_eq!(result, JsonValue::Bool(true));

        // int
        let result = value_to_json(&Value::int(42, span), span).unwrap();
        assert_eq!(result, serde_json::json!(42));

        // float
        let result = value_to_json(&Value::float(3.5, span), span).unwrap();
        assert_eq!(result, serde_json::json!(3.5));

        // string
        let result = value_to_json(&Value::string("hello", span), span).unwrap();
        assert_eq!(result, JsonValue::String("hello".to_string()));
    }

    #[test]
    fn test_value_to_json_list() {
        let span = Span::test_data();
        let value = Value::list(
            vec![
                Value::int(1, span),
                Value::int(2, span),
                Value::int(3, span),
            ],
            span,
        );
        let result = value_to_json(&value, span).unwrap();
        assert_eq!(result, serde_json::json!([1, 2, 3]));
    }

    #[test]
    fn test_value_to_json_record() {
        let span = Span::test_data();
        let value = Value::test_record(record! {
            "name" => Value::string("test", span),
            "count" => Value::int(5, span),
        });
        let result = value_to_json(&value, span).unwrap();

        assert_eq!(result["name"], "test");
        assert_eq!(result["count"], 5);
    }

    #[test]
    fn test_json_to_nu_value_primitives() {
        let span = Span::test_data();

        // null
        let result = json_to_nu_value(&JsonValue::Null, span).unwrap();
        assert!(matches!(result, Value::Nothing { .. }));

        // bool
        let result = json_to_nu_value(&JsonValue::Bool(false), span).unwrap();
        assert_eq!(result, Value::bool(false, span));

        // integer
        let result = json_to_nu_value(&serde_json::json!(99), span).unwrap();
        assert_eq!(result, Value::int(99, span));

        // string
        let result = json_to_nu_value(&JsonValue::String("world".into()), span).unwrap();
        assert_eq!(result, Value::string("world", span));
    }

    #[test]
    fn test_json_to_nu_value_array() {
        let span = Span::test_data();
        let json = serde_json::json!(["a", "b"]);
        let result = json_to_nu_value(&json, span).unwrap();

        if let Value::List { vals, .. } = result {
            assert_eq!(vals.len(), 2);
            assert_eq!(vals[0], Value::string("a", span));
            assert_eq!(vals[1], Value::string("b", span));
        } else {
            panic!("Expected list");
        }
    }

    #[test]
    fn test_json_to_nu_value_object() {
        let span = Span::test_data();
        let json = serde_json::json!({ "key": "value" });
        let result = json_to_nu_value(&json, span).unwrap();

        if let Value::Record { val, .. } = result {
            assert_eq!(val.get("key"), Some(&Value::string("value", span)));
        } else {
            panic!("Expected record");
        }
    }
}

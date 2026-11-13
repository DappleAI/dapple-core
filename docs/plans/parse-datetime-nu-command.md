# parse-datetime Nushell Command

**Dependencies:** `winnow-datetime`, `winnow-rfc3339`, `winnow-iso8601`, `winnow-rfc9557`, `chrono`

## Objective

Add a `parse-datetime` Nushell command to dapple-prompt-flow that parses datetime strings using winnow-datetime parsers and returns Nushell's native `datetime` type. Auto-detects format by trying RFC3339, ISO8601, and RFC9557 in order.

## Command Signature

```nushell
# Auto-detect format (tries rfc3339 → iso8601 → rfc9557)
"2024-01-15T10:30:00Z" | parse-datetime
# → 2024-01-15T10:30:00+00:00 (native nu datetime)

# Works with native nu date commands
"2024-01-15T10:30:00Z" | parse-datetime | format date "%Y-%m-%d"
# → "2024-01-15"

"2024-01-15T10:30:00Z" | parse-datetime | $in + 1day
# → 2024-01-16T10:30:00+00:00

# Force specific format
"2024-01-15T10:30:00Z" | parse-datetime --format iso8601

# Get detailed record instead of datetime
"2024-01-15T10:30:00Z" | parse-datetime --record
# → { format: "rfc3339", year: 2024, month: 1, day: 15, ... }
```

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    dapple-prompt-flow                       │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌─────────────────────────────────────────────────────┐   │
│  │              ParseDatetimeCommand                    │   │
│  │              (nu Command)                            │   │
│  │                                                      │   │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  │   │
│  │  │ winnow-     │  │ winnow-     │  │ winnow-     │  │   │
│  │  │ rfc3339     │  │ iso8601     │  │ rfc9557     │  │   │
│  │  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘  │   │
│  │         │                │                │         │   │
│  │         └────────────────┼────────────────┘         │   │
│  │                          ▼                          │   │
│  │                   winnow-datetime                   │   │
│  │                   (common AST)                      │   │
│  │                          │                          │   │
│  │                          ▼                          │   │
│  │                       jiff                          │   │
│  │                   (Rust datetime)                   │   │
│  │                          │                          │   │
│  │                          ▼                          │   │
│  │                   JSON serialization                │   │
│  │                   (nu Value::Record)                │   │
│  └─────────────────────────────────────────────────────┘   │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## Implementation Tasks

### 1. Add Dependencies to Cargo.toml

```toml
# crates/dapple-prompt-flow/Cargo.toml
[dependencies]
winnow-datetime = "0.x"
winnow-rfc3339 = "0.x"
winnow-iso8601 = "0.x"
winnow-rfc9557 = "0.x"
jiff = { version = "0.x", features = ["serde"] }
```

### 2. Create DateTime Parsing Module

```rust
// crates/dapple-prompt-flow/src/datetime.rs

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveTime, TimeZone, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatetimeFormat {
    Rfc3339,
    Iso8601,
    Rfc9557,
}

impl DatetimeFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Rfc3339 => "rfc3339",
            Self::Iso8601 => "iso8601",
            Self::Rfc9557 => "rfc9557",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "rfc3339" => Some(Self::Rfc3339),
            "iso8601" => Some(Self::Iso8601),
            "rfc9557" => Some(Self::Rfc9557),
            _ => None,
        }
    }
}

/// Result of parsing - always converts to chrono DateTime<FixedOffset>
/// for compatibility with Nushell's Value::Date
#[derive(Debug, Clone)]
pub struct ParseResult {
    pub datetime: DateTime<FixedOffset>,
    pub format: DatetimeFormat,
    /// Original components for --record output
    pub components: DatetimeComponents,
}

#[derive(Debug, Clone)]
pub struct DatetimeComponents {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub nanosecond: u32,
    pub timezone_offset_seconds: i32,
}

impl ParseResult {
    /// Convert to nu-compatible record
    pub fn to_record(&self) -> serde_json::Value {
        serde_json::json!({
            "format": self.format.as_str(),
            "value": self.datetime.to_rfc3339(),
            "year": self.components.year,
            "month": self.components.month,
            "day": self.components.day,
            "hour": self.components.hour,
            "minute": self.components.minute,
            "second": self.components.second,
            "nanosecond": self.components.nanosecond,
            "timezone_offset_seconds": self.components.timezone_offset_seconds,
        })
    }
}

#[derive(Debug)]
pub struct ParseError {
    pub input: String,
    pub tried_formats: Vec<DatetimeFormat>,
    pub errors: Vec<String>,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Failed to parse '{}' as datetime. Tried: {}",
            self.input,
            self.tried_formats.iter().map(|f| f.as_str()).collect::<Vec<_>>().join(", ")
        )
    }
}

/// Try to parse with a specific format
pub fn parse_with_format(input: &str, format: DatetimeFormat) -> Result<ParseResult, String> {
    match format {
        DatetimeFormat::Rfc3339 => parse_rfc3339(input),
        DatetimeFormat::Iso8601 => parse_iso8601(input),
        DatetimeFormat::Rfc9557 => parse_rfc9557(input),
    }
}

/// Auto-detect format and parse (tries rfc3339 → iso8601 → rfc9557)
pub fn parse_auto(input: &str) -> Result<ParseResult, ParseError> {
    let formats = [
        DatetimeFormat::Rfc3339,
        DatetimeFormat::Iso8601,
        DatetimeFormat::Rfc9557,
    ];

    let mut errors = Vec::new();

    for format in formats {
        match parse_with_format(input, format) {
            Ok(result) => return Ok(result),
            Err(e) => errors.push(format!("{}: {}", format.as_str(), e)),
        }
    }

    Err(ParseError {
        input: input.to_string(),
        tried_formats: formats.to_vec(),
        errors,
    })
}

fn parse_rfc3339(input: &str) -> Result<ParseResult, String> {
    use winnow_rfc3339::datetime;

    let parsed = datetime(input)
        .map_err(|e| format!("RFC3339 parse error: {}", e))?;

    let chrono_dt = convert_ast_to_chrono(&parsed)?;
    let components = extract_components(&parsed);

    Ok(ParseResult {
        datetime: chrono_dt,
        format: DatetimeFormat::Rfc3339,
        components,
    })
}

fn parse_iso8601(input: &str) -> Result<ParseResult, String> {
    use winnow_iso8601::datetime;

    let parsed = datetime(input)
        .map_err(|e| format!("ISO8601 parse error: {}", e))?;

    let chrono_dt = convert_ast_to_chrono(&parsed)?;
    let components = extract_components(&parsed);

    Ok(ParseResult {
        datetime: chrono_dt,
        format: DatetimeFormat::Iso8601,
        components,
    })
}

fn parse_rfc9557(input: &str) -> Result<ParseResult, String> {
    use winnow_rfc9557::datetime;

    let parsed = datetime(input)
        .map_err(|e| format!("RFC9557 parse error: {}", e))?;

    let chrono_dt = convert_ast_to_chrono(&parsed)?;
    let components = extract_components(&parsed);

    Ok(ParseResult {
        datetime: chrono_dt,
        format: DatetimeFormat::Rfc9557,
        components,
    })
}

/// Convert winnow-datetime AST to chrono DateTime<FixedOffset>
fn convert_ast_to_chrono(ast: &winnow_datetime::DateTime) -> Result<DateTime<FixedOffset>, String> {
    // Extract components from AST and build chrono DateTime
    // The exact API depends on winnow-datetime's AST structure
    // This is a placeholder - implement based on actual AST types
    todo!("Implement winnow-datetime AST to chrono conversion")
}

fn extract_components(ast: &winnow_datetime::DateTime) -> DatetimeComponents {
    // Extract individual components for record output
    todo!("Extract components from winnow-datetime AST")
}
```

### 3. Create ParseDatetimeCommand

```rust
// crates/dapple-prompt-flow/src/commands/parse_datetime.rs

use crate::datetime::{parse_auto, parse_with_format, DatetimeFormat, ParsedDatetime};
use nu_engine::command_prelude::*;

#[derive(Clone)]
pub struct ParseDatetimeCommand;

impl Command for ParseDatetimeCommand {
    fn name(&self) -> &str {
        "parse-datetime"
    }

    fn signature(&self) -> Signature {
        Signature::build("parse-datetime")
            .input_output_types(vec![
                (Type::String, Type::Date),
                (Type::String, Type::Record(vec![])),  // with --record
            ])
            .named(
                "format",
                SyntaxShape::String,
                "Force specific format: rfc3339, iso8601, or rfc9557",
                Some('f'),
            )
            .switch(
                "record",
                "Return detailed record instead of datetime",
                Some('r'),
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Parse a datetime string using winnow-datetime parsers."
    }

    fn extra_description(&self) -> &str {
        r#"Parses datetime strings and returns a structured record with jiff datetime components.

By default, auto-detects the format by trying:
1. RFC3339 (e.g., "2024-01-15T10:30:00Z")
2. ISO8601 (e.g., "2024-01-15", "10:30:00", "P1D")
3. RFC9557 (e.g., timestamps with extensions)

The output includes the parsed value, format detected, and individual components
(year, month, day, hour, minute, second, etc.) for easy access.

Examples:
  "2024-01-15T10:30:00Z" | parse-datetime
  → { type: "timestamp", format: "rfc3339", value: "2024-01-15T10:30:00Z", ... }

  "2024-01-15" | parse-datetime --date-only
  → { type: "date", format: "iso8601", year: 2024, month: 1, day: 15, ... }
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["datetime", "date", "time", "parse", "rfc3339", "iso8601", "timestamp"]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![
            Example {
                description: "Parse an RFC3339 timestamp",
                example: r#""2024-01-15T10:30:00Z" | parse-datetime"#,
                result: None,
            },
            Example {
                description: "Parse a date with explicit format",
                example: r#""2024-01-15" | parse-datetime --format iso8601"#,
                result: None,
            },
            Example {
                description: "Extract just the year",
                example: r#""2024-01-15T10:30:00Z" | parse-datetime | get year"#,
                result: Some(Value::test_int(2024)),
            },
            Example {
                description: "Parse time only",
                example: r#""14:30:00" | parse-datetime --time-only"#,
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
        let format_str: Option<String> = call.get_flag(engine_state, stack, "format")?;
        let as_record: bool = call.has_flag(engine_state, stack, "record")?;

        // Convert input to string
        let input_value = input.into_value(span)?;
        let text = input_value.as_str().map_err(|_| ShellError::GenericError {
            error: "Input must be a string".into(),
            msg: "Expected string input".into(),
            span: Some(span),
            help: Some("Pipe a datetime string to parse".into()),
            inner: vec![],
        })?.to_string();

        // Determine format
        let explicit_format = format_str
            .as_ref()
            .and_then(|s| DatetimeFormat::from_str(s));

        // Parse
        let result = if let Some(fmt) = explicit_format {
            parse_with_format(&text, fmt).map_err(|e| ShellError::GenericError {
                error: "Parse failed".into(),
                msg: e,
                span: Some(span),
                help: None,
                inner: vec![],
            })?
        } else {
            // Auto-detect
            parse_auto(&text).map_err(|e| ShellError::GenericError {
                error: "Parse failed".into(),
                msg: e.to_string(),
                span: Some(span),
                help: Some("Supported formats: RFC3339, ISO8601, RFC9557".into()),
                inner: vec![],
            })?
        };

        // Return based on --record flag
        if as_record {
            let json = result.to_record();
            let value = json_to_nu_value(&json, span);
            Ok(PipelineData::Value(value, None))
        } else {
            // Return native nu datetime (Value::Date)
            Ok(PipelineData::Value(Value::date(result.datetime, span), None))
        }
    }
}

fn json_to_nu_value(json: &serde_json::Value, span: Span) -> Value {
    match json {
        serde_json::Value::Null => Value::nothing(span),
        serde_json::Value::Bool(b) => Value::bool(*b, span),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::int(i, span)
            } else if let Some(f) = n.as_f64() {
                Value::float(f, span)
            } else {
                Value::string(n.to_string(), span)
            }
        }
        serde_json::Value::String(s) => Value::string(s, span),
        serde_json::Value::Array(arr) => {
            let vals: Vec<Value> = arr.iter().map(|v| json_to_nu_value(v, span)).collect();
            Value::list(vals, span)
        }
        serde_json::Value::Object(obj) => {
            let mut record = Record::new();
            for (k, v) in obj {
                record.push(k, json_to_nu_value(v, span));
            }
            Value::record(record, span)
        }
    }
}
```

### 4. Register Command

```rust
// crates/dapple-prompt-flow/src/commands/mod.rs

pub mod parse_datetime;  // Add this

pub fn register_dapple_commands(engine_state: &mut EngineState) {
    // ... existing commands ...

    // Add parse-datetime command
    working_set.add_decl(Box::new(parse_datetime::ParseDatetimeCommand));

    engine_state.merge_delta(working_set.render());
}
```

### 5. Update lib.rs

```rust
// crates/dapple-prompt-flow/src/lib.rs

pub mod datetime;  // Add this
```

## Output Format

### Default: Native Nushell datetime

Returns `Value::Date` wrapping `chrono::DateTime<FixedOffset>`:

```nushell
"2024-01-15T10:30:00Z" | parse-datetime
# → 2024-01-15T10:30:00+00:00

# Integrates with all nu date commands
$dt | format date "%A, %B %d %Y"  # → "Monday, January 15 2024"
$dt | $in + 1week                  # → datetime arithmetic
$dt | date to-record               # → nu's built-in record conversion
```

### With `--record` flag: Detailed record

```nushell
"2024-01-15T10:30:00Z" | parse-datetime --record
```

```json
{
  "format": "rfc3339",
  "value": "2024-01-15T10:30:00Z",
  "year": 2024,
  "month": 1,
  "day": 15,
  "hour": 10,
  "minute": 30,
  "second": 0,
  "nanosecond": 0,
  "timezone_offset_seconds": 0
}
```

## Files to Create/Modify

| File | Action | Description |
|------|--------|-------------|
| `Cargo.toml` | Modify | Add winnow-datetime, jiff dependencies |
| `src/datetime.rs` | Create | Parsing logic, jiff conversion |
| `src/commands/parse_datetime.rs` | Create | Nu command |
| `src/commands/mod.rs` | Modify | Register command |
| `src/lib.rs` | Modify | Export datetime module |

## Testing

### Unit Tests

```rust
#[test]
fn test_parse_rfc3339() {
    let (parsed, format) = parse_auto("2024-01-15T10:30:00Z").unwrap();
    assert_eq!(format, DatetimeFormat::Rfc3339);
    assert!(matches!(parsed, ParsedDatetime::Timestamp(_)));
}

#[test]
fn test_parse_iso8601_date() {
    let (parsed, format) = parse_auto("2024-01-15").unwrap();
    assert_eq!(format, DatetimeFormat::Iso8601);
    assert!(matches!(parsed, ParsedDatetime::Date(_)));
}

#[test]
fn test_parse_failure() {
    let result = parse_auto("not a date");
    assert!(result.is_err());
}
```

### Manual Test (Nushell)

```nushell
# RFC3339 timestamp
"2024-01-15T10:30:00Z" | parse-datetime
# → { type: "timestamp", format: "rfc3339", ... }

# Extract components
"2024-01-15T10:30:00Z" | parse-datetime | get year
# → 2024

# ISO8601 date
"2024-01-15" | parse-datetime
# → { type: "date", format: "iso8601", year: 2024, month: 1, day: 15, ... }

# Force format
"2024-01-15T10:30:00+05:00" | parse-datetime --format rfc9557

# Time only
"14:30:00" | parse-datetime --time-only
```

## Acceptance Criteria

- [ ] Dependencies added (winnow-datetime family, chrono)
- [ ] `parse-datetime` command registered
- [ ] Auto-detects format (rfc3339 → iso8601 → rfc9557)
- [ ] `--format` flag forces specific parser
- [ ] Default output is native `Value::Date` (chrono DateTime)
- [ ] `--record` flag returns detailed component record
- [ ] Works with nu date commands (`format date`, `+ 1day`, etc.)
- [ ] Error message lists tried formats on failure

## Notes

- This is pure flow, no conduit HTTP call needed
- winnow-datetime AST converts to chrono `DateTime<FixedOffset>`
- Returns native nu datetime for seamless integration with nu date commands
- RFC3339 is tried first as it's the most common web format
- `--record` provides detailed components when needed for inspection

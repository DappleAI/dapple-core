# Security Model

> **Audience:** Backend Developers · Platform Engineers

The StepEngine enforces security policies to prevent dangerous operations in Nushell scripts.

## Security Modes

| Mode | Description |
|------|-------------|
| **Permissive** (default) | Allows all internal Nushell commands; blocks external commands |
| **Allowlist** | Only allows explicitly listed commands |

## Always Blocked

Regardless of mode, these are always blocked:

| Operation | Error |
|-----------|-------|
| External commands (`^curl`, `^ls`) | `ExternalCommandNotAllowed` |
| Environment mutation | `EnvMutationNotAllowed` |
| Plugin commands | `PluginNotAllowed` |
| Oversized metadata | `DisallowedCommand` (with size message) |

## Allowlist Commands (Standard)

When using allowlist mode, these commands are permitted:

### Core Language

- `if`, `else`, `match`, `for`, `while`, `loop`
- `break`, `continue`, `return`, `try`, `catch`
- `def`, `let`, `mut`, `const`

### Data Manipulation

- `echo`, `print`, `get`, `select`, `reject`, `where`, `filter`
- `each`, `par-each`, `reduce`, `fold`, `flatten`, `transpose`
- `group-by`, `sort-by`, `uniq`, `uniq-by`
- `first`, `last`, `take`, `skip`, `drop`, `reverse`
- `length`, `range`, `enumerate`, `zip`, `merge`
- `insert`, `update`, `upsert`, `append`, `prepend`
- `wrap`, `unwrap`, `default`, `compact`

### String Operations

- `str`, `str join`, `str trim`, `str length`, `str replace`
- `str substring`, `str contains`, `str starts-with`, `str ends-with`
- `str index-of`, `str upcase`, `str downcase`, `str capitalize`
- `split`, `split row`, `split column`, `lines`, `parse`, `format`, `fill`

### Type Conversions

- `into`, `into string`, `into int`, `into float`, `into bool`
- `into record`, `into binary`

### JSON/Data Format

- `from json`, `to json`, `from csv`, `to csv`
- `from yaml`, `to yaml`, `from toml`, `to toml`

### Math

- `math`, `math abs`, `math ceil`, `math floor`, `math round`
- `math sqrt`, `math sum`, `math avg`, `math min`, `math max`

### Comparison/Logic

- `is-empty`, `is-not-empty`, `all`, `any`, `not`

### Date/Time

- `date now`, `date format`, `date to-record`

### Utility

- `describe`, `metadata`, `debug`, `error make`, `null`, `ignore`, `collect`
- `headers`, `values`, `columns`, `items`

### Dapple Commands

- `prompt`, `prompt send`, `prompt retry`, `prompt reset`
- `madlib`, `summarize`, `json-schema`

## Security Validation

The security policy validates the AST before execution:

```rust
pub struct SecurityPolicy {
    mode: SecurityMode,
    allowed_commands: HashSet<String>,
}

impl SecurityPolicy {
    pub fn validate(&self, block: &Block) -> Result<(), SecurityError> {
        for pipeline in &block.pipelines {
            for element in &pipeline.elements {
                self.validate_element(element)?;
            }
        }
        Ok(())
    }

    fn validate_element(&self, element: &PipelineElement) -> Result<(), SecurityError> {
        match element {
            PipelineElement::External { .. } => {
                Err(SecurityError::ExternalCommandNotAllowed)
            }
            PipelineElement::Expression(expr) => {
                self.validate_expression(expr)
            }
            // ...
        }
    }
}
```

## Error Types

```rust
pub enum SecurityError {
    /// External command (^cmd) attempted
    ExternalCommandNotAllowed(String),

    /// Environment mutation attempted
    EnvMutationNotAllowed,

    /// Plugin command attempted
    PluginNotAllowed(String),

    /// Command not in allowlist
    DisallowedCommand(String),
}
```

## Configuration

Security policy is configured per-StepEngine:

```rust
let policy = SecurityPolicy::builder()
    .mode(SecurityMode::Allowlist)
    .allow_commands(STANDARD_ALLOWLIST)
    .allow_custom(&["my-custom-cmd"])
    .build();

let engine = StepEngine::new(policy);
```

## Best Practices

### For Task Authors

1. **Use built-in commands** — Avoid needing external tools
2. **Don't mutate environment** — Use parameters instead
3. **Handle data transformations in Nushell** — Avoid shell escapes

### For Platform Operators

1. **Use Allowlist mode in production** — Permissive is for development
2. **Audit custom commands** — Review before adding to allowlist
3. **Monitor blocked attempts** — Log security violations
4. **Update allowlist conservatively** — New commands need review

//! Nushell Plugin Support
//!
//! This module documents how to extend the Nushell engine with custom plugins.
//!
//! # Overview
//!
//! Nushell plugins are external processes that communicate with the main engine
//! via a protocol. They allow adding custom commands without modifying the core engine.
//!
//! # Security Considerations
//!
//! By default, plugins are NOT allowed in the `SecurityPolicy`. To enable plugins:
//! 1. Add the plugin to the engine's plugin registry
//! 2. Update the security policy to allow the specific plugin command
//!
//! # Example: Adding a Custom Command
//!
//! There are two approaches to extending Nushell:
//!
//! ## Approach 1: Built-in Commands (Recommended for Internal Use)
//!
//! For commands that should always be available, you can register them directly
//! with the engine state. This is faster than plugins as there's no IPC overhead.
//!
//! ```ignore
//! use nu_protocol::engine::{Command, EngineState, StateWorkingSet};
//! use nu_protocol::{Category, PipelineData, ShellError, Signature, Type, Value};
//!
//! #[derive(Clone)]
//! pub struct DappleEcho;
//!
//! impl Command for DappleEcho {
//!     fn name(&self) -> &str {
//!         "dapple-echo"
//!     }
//!
//!     fn signature(&self) -> Signature {
//!         Signature::build("dapple-echo")
//!             .input_output_types(vec![(Type::Any, Type::String)])
//!             .category(Category::Custom("dapple".into()))
//!     }
//!
//!     fn usage(&self) -> &str {
//!         "Echo input with a dapple prefix"
//!     }
//!
//!     fn run(
//!         &self,
//!         _engine_state: &EngineState,
//!         _stack: &mut Stack,
//!         _call: &Call,
//!         input: PipelineData,
//!     ) -> Result<PipelineData, ShellError> {
//!         let value = input.into_value(Span::unknown())?;
//!         let output = format!("[dapple] {}", value.to_expanded_string(" ", &Config::default()));
//!         Ok(PipelineData::Value(Value::string(output, Span::unknown()), None))
//!     }
//! }
//!
//! // Register the command:
//! fn register_dapple_commands(engine_state: &mut EngineState) {
//!     let mut working_set = StateWorkingSet::new(engine_state);
//!     working_set.add_decl(Box::new(DappleEcho));
//!     engine_state.merge_delta(working_set.render());
//! }
//! ```
//!
//! ## Approach 2: External Plugins
//!
//! For commands that should be isolated or developed separately, use the nu-plugin crate.
//! External plugins run as separate processes and communicate via stdin/stdout.
//!
//! To create an external plugin:
//!
//! 1. Create a new binary crate with `nu-plugin` dependency
//! 2. Implement the `Plugin` trait
//! 3. Build and register the plugin with `plugin add`
//!
//! ```ignore
//! // In a separate crate: dapple-nu-plugin/src/main.rs
//! use nu_plugin::{Plugin, PluginCommand, SimplePluginCommand};
//! use nu_protocol::{Category, LabeledError, Signature, Type, Value};
//!
//! struct DapplePlugin;
//!
//! impl Plugin for DapplePlugin {
//!     fn version(&self) -> String {
//!         env!("CARGO_PKG_VERSION").into()
//!     }
//!
//!     fn commands(&self) -> Vec<Box<dyn PluginCommand<Plugin = Self>>> {
//!         vec![Box::new(DappleTransform)]
//!     }
//! }
//!
//! struct DappleTransform;
//!
//! impl SimplePluginCommand for DappleTransform {
//!     type Plugin = DapplePlugin;
//!
//!     fn name(&self) -> &str {
//!         "dapple transform"
//!     }
//!
//!     fn signature(&self) -> Signature {
//!         Signature::build("dapple transform")
//!             .input_output_types(vec![(Type::Any, Type::Any)])
//!             .category(Category::Custom("dapple".into()))
//!     }
//!
//!     fn usage(&self) -> &str {
//!         "Transform data using dapple"
//!     }
//!
//!     fn run(
//!         &self,
//!         _plugin: &DapplePlugin,
//!         _engine: &nu_plugin::EngineInterface,
//!         _call: &nu_plugin::EvaluatedCall,
//!         input: &Value,
//!     ) -> Result<Value, LabeledError> {
//!         // Transform the input
//!         Ok(input.clone())
//!     }
//! }
//!
//! fn main() {
//!     nu_plugin::serve_plugin(&DapplePlugin, nu_plugin::MsgPackSerializer)
//! }
//! ```
//!
//! # Plugin Dependencies
//!
//! To use external plugins, add these dependencies:
//!
//! ```toml
//! [dependencies]
//! nu-plugin = "0.100"
//! ```
//!
//! # Current Status
//!
//! Plugin support is documented but not enabled by default. The security policy
//! blocks external commands and plugins to maintain a sandboxed execution environment.
//!
//! To enable plugins in the future:
//! 1. Add a `PluginRegistry` to `StepEngine`
//! 2. Load approved plugins at engine creation time
//! 3. Update `SecurityPolicy` to track allowed plugin commands

/// Marker module for future plugin functionality
pub mod _future {
    /// Placeholder for plugin registry (not yet implemented)
    pub struct PluginRegistry {
        // Will hold registered plugins and their allowed commands
    }

    impl PluginRegistry {
        /// Create an empty plugin registry
        pub fn new() -> Self {
            Self {}
        }
    }

    impl Default for PluginRegistry {
        fn default() -> Self {
            Self::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_registry_creation() {
        let registry = _future::PluginRegistry::new();
        // Just verify it compiles and creates without panic
        let _ = registry;
    }
}

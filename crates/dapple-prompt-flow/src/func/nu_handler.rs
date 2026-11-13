use async_trait::async_trait;
use dapple_data::DbPool;
use dataflow_rs::engine::message::{Change, Message};
use dataflow_rs::engine::{AsyncFunctionHandler, FunctionConfig};
use datalogic_rs::DataLogic;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::engine::{NuOutput, StepEngine};

#[allow(dead_code)]
pub struct NuFunction {
    pool: Option<DbPool>,
    engine: Arc<RwLock<StepEngine>>,
}

impl NuFunction {
    pub fn new_with_pool(pool: DbPool) -> Self {
        Self {
            pool: Some(pool),
            engine: Arc::new(RwLock::new(StepEngine::new())),
        }
    }

    pub fn new() -> Self {
        Self {
            pool: None,
            engine: Arc::new(RwLock::new(StepEngine::new())),
        }
    }

    pub fn new_with_engine(engine: Arc<RwLock<StepEngine>>) -> Self {
        Self { pool: None, engine }
    }
}

impl Default for NuFunction {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AsyncFunctionHandler for NuFunction {
    async fn execute(
        &self,
        message: &mut Message,
        config: &FunctionConfig,
        _datalogic: Arc<DataLogic>,
    ) -> dataflow_rs::Result<(usize, Vec<Change>)> {
        let input = match config {
            FunctionConfig::Custom { input, .. } => input,
            _ => {
                return Err(dataflow_rs::engine::error::DataflowError::Validation(
                    "invalid config for nu function".to_string(),
                ));
            }
        };

        let script = input.get("script").and_then(Value::as_str).unwrap_or("");
        let input_path = input.get("input_path").and_then(Value::as_str);
        let output_path = input
            .get("output_path")
            .and_then(Value::as_str)
            .unwrap_or("data.nu_output");

        // Resolve input value from message.data if provided. The default
        // (no input_path) borrows the whole message data — the engine converts
        // it straight to a nu Value, so no owned copy of the multi-MB journey
        // context is made here.
        let input_val: &Value = if let Some(ip) = input_path {
            if let Some(key) = ip.strip_prefix("data.") {
                message.data().get(key).unwrap_or(&Value::Null)
            } else if let Some(key) = ip.strip_prefix("metadata.") {
                message.metadata().get(key).unwrap_or(&Value::Null)
            } else {
                &Value::Null
            }
        } else {
            // If no input_path, pass the entire data as input
            message.data()
        };

        // Execute via StepEngine
        let mut engine = self.engine.write().await;
        log::debug!(
            "NuFunction: executing script (len={}), input_val keys={:?}, metadata keys={:?}",
            script.len(),
            input_val.as_object().map(|o| o.keys().collect::<Vec<_>>()),
            message
                .metadata()
                .as_object()
                .map(|o| o.keys().collect::<Vec<_>>()),
        );
        let result = engine.execute_script(script, input_val, Some(message.metadata()));

        match result {
            Ok(NuOutput { payload, signals }) => {
                let mut changes = vec![];

                // Write payload to output_path
                if let Some(k) = output_path.strip_prefix("data.")
                    && let Some(map) = message.data_mut().as_object_mut()
                {
                    map.insert(k.to_string(), payload.clone());
                    changes.push(Change {
                        path: output_path.to_string().into(),
                        old_value: Arc::new(json!(null)),
                        new_value: Arc::new(payload.clone()),
                    });
                }

                // Write signals to metadata.signals.*
                if !signals.is_empty()
                    && let Some(meta_map) = message.metadata_mut().as_object_mut()
                {
                    let signals_obj = meta_map
                        .entry("signals".to_string())
                        .or_insert_with(|| json!({}));
                    if let Some(sig_map) = signals_obj.as_object_mut() {
                        for (key, value) in signals {
                            sig_map.insert(key.clone(), value.clone());
                            changes.push(Change {
                                path: format!("metadata.signals.{}", key).into(),
                                old_value: Arc::new(json!(null)),
                                new_value: Arc::new(value),
                            });
                        }
                    }
                }

                Ok((200, changes))
            }
            Err(e) => {
                log::error!(
                    "NuFunction: script execution failed: {:?}, script_first_100={:?}",
                    e,
                    &script[..script.len().min(100)]
                );
                // dataflow-rs collapses the error to its Display string, so the retryability
                // class rides the message metadata instead. ROOT of metadata, never inside
                // `signals` — the external /task_attempts surface projects `signals` verbatim.
                // The dispatch runner's failure arm copies it onto the failed event, where
                // the auto-dispatch retry gate reads it.
                if let Some(meta_map) = message.metadata_mut().as_object_mut() {
                    meta_map.insert("error_class".to_string(), json!(e.error_class().as_str()));
                }
                Err(
                    dataflow_rs::engine::error::DataflowError::function_execution(
                        e.to_string(),
                        None,
                    ),
                )
            }
        }
    }
}

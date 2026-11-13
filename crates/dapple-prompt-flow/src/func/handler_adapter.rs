use async_trait::async_trait;
use dataflow_rs::AsyncFunctionHandler;
use std::sync::Arc;

/// Adapter that allows using `Arc<dyn AsyncFunctionHandler>` where `Box<dyn AsyncFunctionHandler>` is required.
pub struct ArcHandler(pub Arc<dyn AsyncFunctionHandler + Send + Sync>);

#[async_trait]
impl AsyncFunctionHandler for ArcHandler {
    async fn execute(
        &self,
        message: &mut dataflow_rs::engine::message::Message,
        config: &dataflow_rs::engine::FunctionConfig,
        datalogic: std::sync::Arc<datalogic_rs::DataLogic>,
    ) -> dataflow_rs::Result<(usize, Vec<dataflow_rs::engine::message::Change>)> {
        self.0.execute(message, config, datalogic).await
    }
}

use std::collections::HashMap;

/// Convert an Arc-based function map into a boxed owned map suitable for `Engine::new`.
pub fn boxed_map_from_arc_map(
    src: &HashMap<String, Arc<dyn AsyncFunctionHandler + Send + Sync>>,
) -> HashMap<String, Box<dyn AsyncFunctionHandler + Send + Sync>> {
    let mut out: HashMap<String, Box<dyn AsyncFunctionHandler + Send + Sync>> = HashMap::new();
    for (k, v) in src.iter() {
        out.insert(k.clone(), Box::new(ArcHandler(v.clone())));
    }
    out
}

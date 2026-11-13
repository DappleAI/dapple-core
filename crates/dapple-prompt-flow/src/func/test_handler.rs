use async_trait::async_trait;
use dataflow_rs::engine::{
    AsyncFunctionHandler, FunctionConfig,
    message::{Change, Message},
};
use datalogic_rs::DataLogic;
use serde_json::json;
use std::sync::Arc;

pub struct TestFunction;

#[async_trait]
impl AsyncFunctionHandler for TestFunction {
    async fn execute(
        &self,
        message: &mut Message,
        _config: &FunctionConfig,
        _datalogic: Arc<DataLogic>,
    ) -> dataflow_rs::Result<(usize, Vec<Change>)> {
        // Read existing value at data.test_executed
        let old = message
            .data()
            .get("test_executed")
            .cloned()
            .unwrap_or(json!(false));

        let new = json!(true);

        // mutate the message data
        if let Some(map) = message.data_mut().as_object_mut() {
            map.insert("test_executed".to_string(), new.clone());
        }

        Ok((
            200,
            vec![Change {
                path: "data.test_executed".into(),
                old_value: std::sync::Arc::new(old),
                new_value: std::sync::Arc::new(new),
            }],
        ))
    }
}

impl Default for TestFunction {
    fn default() -> Self {
        TestFunction
    }
}

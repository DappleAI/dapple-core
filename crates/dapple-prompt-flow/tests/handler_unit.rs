use dataflow_rs::Engine;
use dataflow_rs::Workflow;
use serde_json::json;
use std::collections::HashMap;

use dapple_prompt_flow::func::test_handler::TestFunction;
use dataflow_rs::AsyncFunctionHandler;

#[tokio::test]
async fn test_handler_executes_and_mutates_message() -> Result<(), Box<dyn std::error::Error>> {
    // simple workflow JSON with a single task invoking `test_fn`
    let wf = r#"
    {
        "id": "wf_test",
        "name": "Test WF",
        "tasks": [
            {
                "id": "t1",
                "name": "run test",
                "function": { "name": "test_fn", "input": {} }
            }
        ]
    }
    "#;

    let workflow = Workflow::from_json(wf)?;

    // register our deterministic test function
    let mut functions: HashMap<String, Box<dyn AsyncFunctionHandler + Send + Sync>> =
        HashMap::new();
    functions.insert("test_fn".into(), Box::new(TestFunction));

    let engine = Engine::new(vec![workflow], Some(functions));

    let mut message = dataflow_rs::engine::message::Message::from_value(&json!({}));

    engine.process_message(&mut message).await?;

    // ensure our handler updated the message at data.test_executed
    let val = message
        .data()
        .get("test_executed")
        .cloned()
        .unwrap_or(json!(false));
    assert_eq!(val, json!(true));

    Ok(())
}

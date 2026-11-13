use crate::TemplateRegistry;
use async_trait::async_trait;
use dataflow_rs::engine::{
    AsyncFunctionHandler, FunctionConfig,
    error::DataflowError,
    message::{Change, Message},
};
use datalogic_rs::DataLogic;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::sync::Arc;

// TODO: Prompt Usage struct
#[derive(Clone, Debug, Deserialize, Serialize)]
struct PromptRequest<P, R> {
    prompt: P,
    response: Option<R>,
}

// TODO: needs a token usage value as well
#[derive(Clone, Debug, Deserialize, Serialize)]
struct TextPrompt {
    template: String,
    input: Vec<Value>,
    prompt_text: String,
}

// TODO: needs a token usage value as well
#[derive(Clone, Debug, Deserialize, Serialize)]
struct TextPromptResponse {
    response_text: String,
    response_code: usize,
}

pub struct PromptTextFunction {
    template_registry: TemplateRegistry,
}

#[async_trait]
impl AsyncFunctionHandler for PromptTextFunction {
    async fn execute(
        &self,
        message: &mut Message,
        config: &FunctionConfig,
        _datalogic: Arc<DataLogic>,
    ) -> dataflow_rs::Result<(usize, Vec<Change>)> {
        let input = match config {
            FunctionConfig::Custom { input, .. } => input,
            _ => {
                return Err(DataflowError::Validation(
                    "Invalid configuration type for statistics function".to_string(),
                ));
            }
        };

        // Extract the data path to analyze
        let prompt_template = input
            .get("prompt_template")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                DataflowError::Validation("Unable to find prompt_template".to_string())
            })?;

        let output_path = input
            .get("output_path")
            .and_then(Value::as_str)
            .ok_or_else(|| DataflowError::Validation("Unable to find output_path".to_string()))?;

        let workflow_id = message
            .metadata()
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                DataflowError::Validation("Unable to find workflow id in metadata".to_string())
            })?;

        // TODO: this should have a find template text function here
        // TODO: we need to verify the template is from an acceptable path, this path hasn't really been worked out yet
        let _prompt_template = prompt_template.to_string();

        //TODO: should we register this? Should it be pre-registered with a name?

        let templates = &self.template_registry;

        let mut lookups = &vec![];

        lookups = input
            .get("lookups")
            .and_then(Value::as_array)
            .unwrap_or(lookups);

        let old_value: &Vec<PromptRequest<TextPrompt, TextPromptResponse>> = &message
            .metadata()
            .get("prompt_requests")
            .and_then(Value::as_array)
            .map(|s| {
                s.iter().fold(vec![], |mut acc, x| {
                    acc.push(PromptRequest::deserialize(x.clone()).unwrap());
                    acc
                })
            })
            .unwrap_or_default();

        let mut requests = old_value.clone();
        let mut errors = vec![];

        for lookup in lookups {
            let lookup = lookup.as_object().unwrap();

            println!("lookup: {:#?}", lookup);

            let lookup = self.lookup_values(lookup)?;

            let prompt = match templates.render(workflow_id, lookup) {
                Ok(s) => Ok(s),
                Err(e) => Err(DataflowError::Validation(format!(
                    "Invalid template, gave render error: {}",
                    e
                ))),
            }?;

            let req = self.send_prompt(message, &prompt)?;

            if let Some(response) = req.response.clone() {
                if response.response_code != 200 {
                    errors.push(DataflowError::Validation(
                        "Unexpected response code from prompt call".to_string(),
                    ));
                }
            } else {
                errors.push(DataflowError::Validation(
                    "Missing response from prompt call".to_string(),
                ));
            }

            if errors.is_empty() {
                requests.push(req);
            } else {
                println!("Prompting errors: {:#?}", errors);
                //TODO: this error isn't actually helpful
                return Err(DataflowError::Validation(
                    "Errors occurred while processing prompts".to_string(),
                ));
            }
        }

        println!("Prompt requests: {:#?}", requests);

        let response_texts = requests
            .iter()
            .filter_map(|r| r.response.as_ref())
            .map(|r| r.response_text.clone())
            .collect::<Vec<String>>();

        println!("response_texts: {:#?}", response_texts);

        // TODO: make sure you change this piece as well
        self.set_value_at_path(message, output_path, json!(response_texts.clone()))?;

        let output_path = "data.prompt_requests";
        // Store the results in the message

        Ok((
            200,
            vec![Change {
                path: output_path.to_string().into(),
                old_value: Arc::new(json!(old_value)),
                new_value: Arc::new(json!(requests)),
            }],
        ))
    }
}

impl Default for PromptTextFunction {
    fn default() -> Self {
        Self::new(TemplateRegistry::new(HashMap::default()))
    }
}

impl PromptTextFunction {
    pub fn new(template_registry: TemplateRegistry) -> Self {
        Self { template_registry }
    }

    // This will eventually send a request via Rig
    fn send_prompt(
        &self,
        _message: &Message,
        p: &str,
    ) -> dataflow_rs::Result<PromptRequest<TextPrompt, TextPromptResponse>> {
        //TODO: estimate tokens being sent

        println!("Will eventually run prompt template: {}", p);

        // TODO: run tera
        let p = p.to_string();

        // TODO: run RAG
        // TODO: estimate received tokens

        //TODO: you need to get the template name from the message
        Ok(PromptRequest {
            //TODO: seems like this is the piece that should be coming in as an arg actually
            prompt: TextPrompt {
                template: "TODO".to_string(),
                input: vec![],
                prompt_text: p,
            },
            response: Some(TextPromptResponse {
                response_text: "Beep bop boop, super smart response".to_string(),
                response_code: 200,
            }),
        })
    }

    // TODO: this should take in whatever values are needed to fully interpolate the prompt. An error
    #[allow(dead_code)]
    fn build_prompt(&self, pt: String) -> Value {
        // TODO: interpolate the template in pt:
        // TODO: error out if tera template requested is missing any values
        println!("ignoring prompt template: {}", pt);

        json!({
            "prompt": "This is a placeholder for the prompt text."
        })
    }

    fn lookup_values(&self, m: &Map<String, Value>) -> dataflow_rs::Result<Map<String, Value>> {
        let mut acc = Map::new();

        for (k, v) in m {
            let p = v.get("path").ok_or_else(|| {
                DataflowError::Validation(format!("Missing value for path {}", &k))
            })?;

            acc.insert(k.clone(), p.clone());
        }

        Ok(acc)
    }

    #[allow(dead_code)]
    fn get_object_at_path<'a>(
        &self,
        message: &'a mut Message,
        path: &str,
    ) -> dataflow_rs::Result<&'a Map<String, Value>> {
        let parts: Vec<&str> = path.split('.').collect();

        if parts.len() > 1 {
            return Err(DataflowError::Validation(
                "path depth of greater than 1 not supported yet".to_string(),
            ));
        }
        let target = if parts[0] == "data" {
            Ok(message.data_mut())
        } else if parts[0] == "temp_data" {
            Ok(message.temp_data_mut())
        } else if parts[0] == "metadata" {
            Ok(message.metadata_mut())
        } else {
            //TODO: here we should walk all of the options in priority of temp_data -> data -> metadata
            Err(
                DataflowError::Validation(
                    "paths for templates should be a complete path for now, path searching coming soon.".to_string()
                )
            )
        }?;

        let v = target
            .get(parts[1])
            .and_then(Value::as_object)
            .ok_or_else(|| {
                DataflowError::Validation(format!(
                    "couldn't find object at {}.{}",
                    parts[0], parts[1]
                ))
            })?;

        Ok(v)
    }

    // TODO: this should be moved to a common function trait
    fn set_value_at_path(
        &self,
        message: &mut Message,
        path: &str,
        value: Value,
    ) -> dataflow_rs::Result<()> {
        let parts: Vec<&str> = path.split('.').collect();
        // This is duplicate and belongs in a common trait
        let target = if parts[0] == "data" {
            message.data_mut()
        } else if parts[0] == "temp_data" {
            message.temp_data_mut()
        } else if parts[0] == "metadata" {
            message.metadata_mut()
        } else {
            message.data_mut()
        };

        // Navigate and set the value
        let mut current = target;

        for (i, part) in parts[1..].iter().enumerate() {
            if i == parts.len() - 2 {
                // Last part, set the value
                if current.is_null() {
                    *current = json!({});
                }
                if let Value::Object(map) = current {
                    map.insert(part.to_string(), value.clone());
                }
                break;
            } else {
                // Navigate deeper
                if current.is_null() {
                    *current = json!({});
                }
                if let Value::Object(map) = current {
                    if !map.contains_key(*part) {
                        map.insert(part.to_string(), json!({}));
                    }
                    current = map.get_mut(*part).unwrap();
                }
            }
        }

        Ok(())
    }
}

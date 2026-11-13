pub mod access;
pub mod commands;
pub mod conduit_client;
pub mod directives;
pub mod dispatch_runner;
pub mod edgar_adv;
pub mod edgar_advised;
pub mod edgar_book;
pub mod edgar_comp;
pub mod edgar_cusip;
pub mod edgar_deadlines;
pub mod edgar_dera;
pub mod edgar_form345;
pub mod edgar_fund_linkbase;
pub mod edgar_fund_registry;
pub mod edgar_fund_sections;
pub mod edgar_fund_tags;
pub mod edgar_fund_text;
pub mod edgar_identity;
pub mod edgar_manager;
pub mod edgar_ncen;
pub mod edgar_nmfp;
pub mod edgar_nmfp_dera;
pub mod edgar_nport;
pub mod edgar_portfolio;
pub mod edgar_sections;
pub mod edgar_security;
pub mod edgar_thirteenf;
pub mod edgar_xbrl_dims;
pub mod engine;
pub mod extractors;
pub mod fmt;
pub mod func;
pub mod md;
pub mod media_gateway_client;
pub mod test_support;
pub mod text;

pub use extractors::{enqueue_dataset_extraction, enqueue_pdf_extraction, run_extraction_workers};

use dataflow_rs::{
    AsyncFunctionHandler, Engine, Workflow,
    engine::error::{DataflowError, Result},
};
use serde_json::{Map, Value};
use std::collections::HashMap;

use crate::func::prompt::PromptTextFunction;
use dapple_fluent::substitute;

pub struct TemplateRegistry {
    templates: HashMap<String, String>,
}

impl TemplateRegistry {
    pub fn new(_templates: HashMap<String, &Value>) -> Self {
        TemplateRegistry {
            templates: HashMap::new(),
        }
    }

    pub fn register(&mut self, name: &str, template: &str) -> Result<()> {
        self.templates
            .insert(name.to_string(), template.to_string());
        Ok(())
    }

    pub fn render(&self, name: &str, data: Map<String, Value>) -> Result<String> {
        let template = self
            .templates
            .get(name)
            .ok_or_else(|| DataflowError::Validation(format!("Template '{}' not found", name)))?;

        // Convert Map<String, Value> to HashMap<String, Value>
        let context: HashMap<String, Value> = data.into_iter().collect();
        Ok(substitute(template, &context))
    }
}

pub struct Step {
    pub workflows: Vec<Workflow>,
}

/// Provides an engine with all of the added custom functions avaialble.
impl Step {
    pub fn engine(&self) -> Engine {
        let functions = HashMap::from([(
            "prompt_text".into(),
            Box::new(PromptTextFunction::default()) as Box<dyn AsyncFunctionHandler + Send + Sync>,
        )]);

        // Create the workflow engine (built-in functions are auto-registered)

        // setup the context

        Engine::new(self.workflows.clone(), Some(functions))
    }

    /// Construct an Engine using the provided function map instead of the default.
    pub fn engine_with_functions(
        &self,
        functions: HashMap<String, Box<dyn AsyncFunctionHandler + Send + Sync>>,
    ) -> Engine {
        Engine::new(self.workflows.clone(), Some(functions))
    }
}

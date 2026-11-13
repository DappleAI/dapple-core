use dapple_data::error::DataError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConsumerError {
    #[error("data error: {0}")]
    Data(#[from] DataError),
    #[error("reqwest error: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("serde json error: {0}")]
    SerdeJson(#[from] serde_json::Error),
    #[error("url parsinng error: {0}")]
    UrlParse(#[from] url::ParseError),
    #[error("{0}")]
    Custom(String),
}

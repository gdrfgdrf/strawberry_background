use flutter_rust_bridge::frb;
use crate::domain::models::http_models::HttpClientError;

#[derive(Debug, thiserror::Error)]
pub enum FfiAdapterError {
    #[error("Parameter error: {0}")]
    InvalidParameter(String),
    #[error("Domain error: {0}")]
    DomainError(String),
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("Configuration error: {0}")]
    Configuration(String),
}

impl FfiAdapterError {
    #[frb(ignore)]
    pub fn from_domain_error(err: HttpClientError) -> Self {
        match err {
            HttpClientError::Network(msg) => {
                FfiAdapterError::DomainError(format!("Network: {}", msg))
            }
            HttpClientError::Timeout(dur) => {
                FfiAdapterError::DomainError(format!("Timeout after {:?}", dur))
            }
            HttpClientError::InvalidUrl(url) => {
                FfiAdapterError::DomainError(format!("Invalid URL: {}", url))
            }
            HttpClientError::Serialization(msg) => FfiAdapterError::Serialization(msg),
            HttpClientError::Configuration(msg) => FfiAdapterError::Configuration(msg),
            HttpClientError::InvalidHeader(msg) => {
                FfiAdapterError::DomainError(format!("Invalid Header: {}", msg))
            }
            HttpClientError::Crypto(msg) => {
                FfiAdapterError::DomainError(format!("Crypto: {}", msg))
            }
        }
    }
}

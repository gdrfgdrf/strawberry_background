
use crate::utils::url_component::{encode_component, encode_query_component};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct HttpEndpoint {
    pub path: String,
    pub domain: String,
    pub body: Option<Vec<u8>>,
    pub timeout: Duration,

    pub headers: Option<Vec<(String, String)>>,
    pub path_params: Option<Vec<(String, String)>>,
    pub query_params: Option<Vec<(String, String)>>,

    pub method: HttpMethod,
    pub requires_encryption: bool,
    pub requires_decryption: bool,
    pub user_agent: Option<String>,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum HttpClientError {
    #[error("Network error: {0}")]
    Network(String),
    #[error("Timeout after {0:?}")]
    Timeout(Duration),
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),
    #[error("Invalid Header: {0}")]
    InvalidHeader(String),
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("Configuration error: {0}")]
    Configuration(String),
    #[error("Crypto error: {0}")]
    Crypto(String)
}

impl HttpEndpoint {
    fn combine_path_params_to_path(&self, path: String) -> String {
        if self.path_params.is_none() {
            return path;
        }
        let path_params = self.path_params.as_ref().unwrap();
        if path_params.is_empty() {
            return path;
        }
        let mut path = path;

        path_params.iter().for_each(|(key, value)| {
            let encoded_value = encode_component(value);
            path = path.replace(&format!(":{}", key), &encoded_value);
        });

        path
    }

    fn combine_query_params_to_path(&self, path: String) -> String {
        if self.query_params.is_none() {
            return path;
        }
        let query_params = self.query_params.as_ref().unwrap();
        if query_params.is_empty() {
            return path;
        }

        let encoded: String = query_params
            .iter()
            .map(
                (|(key, value)| {
                    return format!(
                        "{}={}",
                        encode_query_component(key),
                        encode_query_component(value)
                    );
                }),
            )
            .collect::<Vec<String>>()
            .join("&");

        format!("{}?{}", path, encoded)
    }

    pub fn build_url(&self) -> String {
        let url = format!("{}{}", self.domain, self.path);
        let url = self.combine_path_params_to_path(url);
        let url = self.combine_query_params_to_path(url);

        url
    }
}

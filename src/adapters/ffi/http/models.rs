use crate::adapters::ffi::errors::FfiAdapterError;
use crate::domain::models::http_models::{HttpEndpoint, HttpMethod, HttpResponse};
use std::time::Duration;

pub struct FfiHttpEndpoint {
    pub path: String,
    pub domain: String,
    pub body: Option<Vec<u8>>,
    pub timeout_millis: u64,

    pub headers: Option<Vec<(String, String)>>,
    pub path_params: Option<Vec<(String, String)>>,
    pub query_params: Option<Vec<(String, String)>>,

    pub method: String,
    pub requires_encryption: bool,
    pub requires_decryption: bool,
    pub user_agent: Option<String>,
    pub content_type: Option<String>,
}

pub struct FfiHttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl FfiHttpEndpoint {
    pub fn to_domain_endpoint(self) -> Result<HttpEndpoint, FfiAdapterError> {
        let method = match self.method.as_str() {
            "GET" => HttpMethod::Get,
            "POST" => HttpMethod::Post,
            "PUT" => HttpMethod::Put,
            "DELETE" => HttpMethod::Delete,
            _ => return Err(FfiAdapterError::InvalidParameter(self.method)),
        };

        Ok(HttpEndpoint {
            path: self.path,
            domain: self.domain,
            body: self.body,
            timeout: Duration::from_millis(self.timeout_millis),
            headers: self.headers,
            path_params: self.path_params,
            query_params: self.query_params,
            method,
            requires_encryption: self.requires_encryption,
            requires_decryption: self.requires_decryption,
            user_agent: self.user_agent,
            content_type: self.content_type,
        })
    }
}

impl From<HttpResponse> for FfiHttpResponse {
    fn from(domain_resp: HttpResponse) -> Self {
        FfiHttpResponse {
            status: domain_resp.status,
            headers: domain_resp.headers,
            body: domain_resp.body,
        }
    }
}

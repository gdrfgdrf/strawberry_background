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

    pub method: FfiHttpMethod,
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

pub enum FfiHttpMethod {
    Get,
    Post,
    Put,
    Delete,
}

impl Into<HttpMethod> for FfiHttpMethod {
    fn into(self) -> HttpMethod {
        match self {
            FfiHttpMethod::Get => HttpMethod::Get,
            FfiHttpMethod::Post => HttpMethod::Post,
            FfiHttpMethod::Put => HttpMethod::Put,
            FfiHttpMethod::Delete => HttpMethod::Delete
        }
    }
}

impl Into<HttpEndpoint> for FfiHttpEndpoint {
    fn into(self) -> HttpEndpoint {
        HttpEndpoint {
            path: self.path,
            domain: self.domain,
            body: self.body,
            timeout: Duration::from_millis(self.timeout_millis),
            headers: self.headers,
            path_params: self.path_params,
            query_params: self.query_params,
            method: self.method.into(),
            requires_encryption: self.requires_encryption,
            requires_decryption: self.requires_decryption,
            user_agent: self.user_agent,
            content_type: self.content_type,
        }
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

impl FfiHttpEndpoint {
    pub fn new(
        path: String,
        domain: String,
        body: Option<Vec<u8>>,
        timeout_millis: u64,

        headers: Option<Vec<(String, String)>>,
        path_params: Option<Vec<(String, String)>>,
        query_params: Option<Vec<(String, String)>>,

        method: FfiHttpMethod,
        requires_encryption: bool,
        requires_decryption: bool,
        user_agent: Option<String>,
        content_type: Option<String>,
    ) -> FfiHttpEndpoint {
        FfiHttpEndpoint {
            path,
            domain,
            body,
            timeout_millis,
            headers,
            path_params,
            query_params,
            method,
            requires_encryption,
            requires_decryption,
            user_agent,
            content_type
        }
    }
}

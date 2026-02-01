use crate::adapters::ffi::http::models::{FfiHttpEndpoint, FfiHttpResponse};
use crate::service::service_runtime::ServiceRuntime;
use std::sync::Arc;

pub struct ServiceFfiAdapter {
    runtime: Arc<ServiceRuntime>,
}

impl ServiceFfiAdapter {
    pub fn new(runtime: Arc<ServiceRuntime>) -> Self {
        Self { runtime }
    }

    pub async fn execute_http_endpoint(
        &self,
        ffi_endpoint: FfiHttpEndpoint,
    ) -> Result<FfiHttpResponse, String> {
        let http_client = &self.runtime.http_client;
        if http_client.is_none() {
            return Err("http is not configured".to_string());
        }

        let domain_endpoint = ffi_endpoint
            .to_domain_endpoint()
            .map_err(|e| format!("{}", e))?;

        let domain_response = self
            .runtime
            .execute_http(domain_endpoint)
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| format!("{}", e))?;

        Ok(FfiHttpResponse::from(domain_response))
    }
}

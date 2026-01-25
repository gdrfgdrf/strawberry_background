use crate::adapters::ffi::errors::FfiAdapterError;
use crate::adapters::ffi::http::models::{FfiHttpEndpoint, FfiHttpResponse};
use crate::service::service_runtime::ServiceRuntime;
use std::sync::Arc;
use flutter_rust_bridge::frb;

#[frb]
pub struct ServiceFfiAdapter {
    runtime: Arc<ServiceRuntime>,
}

impl ServiceFfiAdapter {
    pub fn new(runtime: Arc<ServiceRuntime>) -> Self {
        Self { runtime }
    }

    #[frb]
    pub async fn execute_http_endpoint(
        &self,
        ffi_endpoint: FfiHttpEndpoint,
    ) -> Result<FfiHttpResponse, FfiAdapterError> {
        let http_client = &self.runtime.http_client;
        if http_client.is_none() {
            return Err(FfiAdapterError::Configuration("http is not configured".to_string()));
        }
        
        let http_client = http_client.as_ref().unwrap().clone();
        let domain_endpoint = ffi_endpoint.to_domain_endpoint()?;

        let domain_response = self
            .runtime
            .clone()
            .execute_async(async move { http_client.execute(domain_endpoint).await })
            .map_err(FfiAdapterError::from_domain_error)?;

        Ok(FfiHttpResponse::from(domain_response))
    }
}

use crate::adapters::ffi::http::models::{FfiHttpEndpoint, FfiHttpResponse};
use crate::service::service_runtime::ServiceRuntime;
use std::sync::Arc;
use flutter_rust_bridge::frb;

#[frb(external)]
pub struct ServiceFfiAdapter {
    #[frb(ignore)]
    runtime: Arc<ServiceRuntime>,
}

#[frb(external)]
impl ServiceFfiAdapter {
    #[frb(ignore)]
    pub fn new(runtime: Arc<ServiceRuntime>) -> Self {
        Self { runtime }
    }

    #[frb(external)]
    pub async fn execute_http_endpoint(
        &self,
        ffi_endpoint: FfiHttpEndpoint,
    ) -> Result<FfiHttpResponse, String> {
        let http_client = &self.runtime.http_client;
        if http_client.is_none() {
            return Err("http is not configured".to_string());
        }
        
        let http_client = http_client.as_ref().unwrap().clone();
        let domain_endpoint = ffi_endpoint.to_domain_endpoint().map_err(|e| "Convert to domain endpoint error".to_string())?;

        let domain_response = self
            .runtime
            .clone()
            .execute_async(async move { http_client.execute(domain_endpoint).await })
            .map_err(|e| e.to_string())?;

        Ok(FfiHttpResponse::from(domain_response))
    }
}

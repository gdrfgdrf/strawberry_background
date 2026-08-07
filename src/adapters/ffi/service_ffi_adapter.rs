use crate::adapters::ffi::http::models::{FfiHttpEndpoint, FfiHttpResponse, FfiHttpStreamResponse};
use crate::adapters::ffi::storage::models::{FfiReadFile, FfiWriteFile};
use crate::domain::models::storage_models::WriteFile;
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
        let domain_endpoint = ffi_endpoint.into();
        let domain_response = self
            .runtime
            .execute_http(domain_endpoint)
            .map_err(|e| e.to_string())?
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;

        Ok(FfiHttpResponse::from(domain_response))
    }

    pub async fn execute_stream_http_endpoint(
        &self,
        ffi_endpoint: FfiHttpEndpoint,
    ) -> Result<FfiHttpStreamResponse, String> {
        let domain_endpoint = ffi_endpoint.into();
        let domain_response = self
            .runtime
            .execute_stream_http(domain_endpoint)
            .map_err(|e| e.to_string())?
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;

        Ok(FfiHttpStreamResponse::from(domain_response))
    }

    pub async fn read_file(&self, ffi_read_file: FfiReadFile) -> Result<Vec<u8>, String> {
        let domain_read_file = ffi_read_file.into();
        let data = self
            .runtime
            .read_file(domain_read_file)
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;

        Ok(data)
    }

    pub async fn write_file(&self, ffi_write_file: FfiWriteFile) -> Result<(), String> {
        let domain_write_file = WriteFile::from(&ffi_write_file);
        let data = self
            .runtime
            .write_file(domain_write_file)
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;

        Ok(data)
    }
}

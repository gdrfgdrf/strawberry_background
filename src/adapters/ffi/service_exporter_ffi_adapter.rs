use crate::adapters::ffi::service_ffi_adapter::ServiceFfiAdapter;
use crate::service::config::RuntimeConfig;
use crate::service::service_runtime::{InitError, ServiceRuntime};
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use tokio::runtime::Runtime;

pub struct ServiceExporterFfiAdapter {
    runtime: Arc<ServiceRuntime>,
}

impl ServiceExporterFfiAdapter {
    pub fn new(runtime: Arc<ServiceRuntime>) -> Self {
        Self { runtime }
    }

    pub fn runtime_ffi_adapter(&self) -> ServiceFfiAdapter {
        ServiceFfiAdapter::new(Arc::clone(&self.runtime))
    }

    pub fn runtime(&self) -> &Arc<ServiceRuntime> {
        &self.runtime
    }
}

pub fn create_service_exporter_ffi_adapter(
    config: RuntimeConfig,
) -> Result<ServiceExporterFfiAdapter, InitError> {
    let runtime = ServiceRuntime::initialize(config)?;
    Ok(ServiceExporterFfiAdapter::new(runtime))
}

pub fn create_service_exporter_ffi_adapter_with_tokio_runtime(
    config: RuntimeConfig,
    tokio_runtime: Arc<AssertUnwindSafe<Runtime>>,
) -> Result<ServiceExporterFfiAdapter, InitError> {
    let runtime = ServiceRuntime::with_tokio_runtime(config, tokio_runtime)?;
    Ok(ServiceExporterFfiAdapter::new(runtime))
}

use crate::service::config::RuntimeConfig;
use crate::service::service_runtime::{InitError, ServiceRuntime};
use std::sync::Arc;
use flutter_rust_bridge::frb;
use crate::adapters::ffi::service_ffi_adapter::ServiceFfiAdapter;

#[frb(external)]
pub struct ServiceExporterFfiAdapter {
    #[frb(ignore)]
    runtime: Arc<ServiceRuntime>,
}

#[frb(external)]
impl ServiceExporterFfiAdapter {
    #[frb(ignore)]
    pub fn new(runtime: Arc<ServiceRuntime>) -> Self {
        Self { runtime }
    }

    #[frb(external)]
    pub fn runtime_ffi_adapter(&self) -> ServiceFfiAdapter {
        ServiceFfiAdapter::new(Arc::clone(&self.runtime))
    }

    #[frb(ignore)]
    pub fn runtime(&self) -> &Arc<ServiceRuntime> {
        &self.runtime
    }
}

#[frb(ignore)]
pub fn create_service_exporter_ffi_adapter(
    config: RuntimeConfig,
) -> Result<ServiceExporterFfiAdapter, InitError> {
    let runtime = ServiceRuntime::initialize(config)?;
    Ok(ServiceExporterFfiAdapter::new(runtime))
}

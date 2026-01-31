use crate::service::config::RuntimeConfig;
use crate::service::service_runtime::{InitError, ServiceRuntime};
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use tokio::runtime::Runtime;

pub struct ServiceExporter {
    runtime: Arc<ServiceRuntime>,
}

impl ServiceExporter {
    pub fn new(runtime: Arc<ServiceRuntime>) -> Self {
        Self { runtime }
    }

    pub fn runtime(&self) -> &Arc<ServiceRuntime> {
        &self.runtime
    }
}

pub fn create_service_exporter(config: RuntimeConfig) -> Result<ServiceExporter, InitError> {
    let runtime = ServiceRuntime::initialize(config)?;
    Ok(ServiceExporter::new(runtime))
}

pub fn create_service_exporter_with_tokio_runtime(
    config: RuntimeConfig,
    tokio_runtime: Arc<AssertUnwindSafe<Runtime>>,
) -> Result<ServiceExporter, InitError> {
    let runtime = ServiceRuntime::with_tokio_runtime(config, tokio_runtime)?;
    Ok(ServiceExporter::new(runtime))
}

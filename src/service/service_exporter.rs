use std::sync::Arc;
use crate::service::config::RuntimeConfig;
use crate::service::service_runtime::{InitError, ServiceRuntime};

pub struct ServiceExporter {
    runtime: Arc<ServiceRuntime>
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
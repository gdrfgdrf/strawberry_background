use crate::service_runtime::config::RuntimeConfig;
use crate::service_runtime::service_exporter::ServiceExporter;
use crate::service_runtime::service_runtime::InitError;

mod adapters;
mod domain;
mod infrastructure;
mod service_runtime;
mod utils;

pub fn initialize(config: RuntimeConfig) -> Result<ServiceExporter, InitError> {
    service_runtime::service_exporter::create_service_exporter(config)
}

pub fn init_default() -> Result<ServiceExporter, InitError> {
    let config = RuntimeConfig::default();
    initialize(config)
}

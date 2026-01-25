use crate::service_runtime::config::RuntimeConfig;
use crate::service_runtime::service_exporter::ServiceExporter;
use crate::service_runtime::service_runtime::InitError;

pub mod adapters;
pub mod domain;
pub mod infrastructure;
pub mod service_runtime;
pub mod utils;

pub fn initialize(config: RuntimeConfig) -> Result<ServiceExporter, InitError> {
    service_runtime::service_exporter::create_service_exporter(config)
}

pub fn init_default() -> Result<ServiceExporter, InitError> {
    let config = RuntimeConfig::default();
    initialize(config)
}

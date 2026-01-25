use crate::service::config::RuntimeConfig;
use crate::service::service_exporter::ServiceExporter;
use crate::service::service_runtime::InitError;

pub mod adapters;
pub mod domain;
pub mod infrastructure;
pub mod service;
pub mod utils;

pub fn initialize(config: RuntimeConfig) -> Result<ServiceExporter, InitError> {
    service::service_exporter::create_service_exporter(config)
}

pub fn init_default() -> Result<ServiceExporter, InitError> {
    let config = RuntimeConfig::default();
    initialize(config)
}

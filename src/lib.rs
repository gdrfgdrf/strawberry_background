pub mod adapters;
pub mod domain;
pub mod infrastructure;
pub mod service;
pub mod utils;
pub mod rkyv;
pub mod superstructure;
pub mod monitor;

use std::sync::Arc;
use crate::domain::traits::monitor_traits::Monitor;
use crate::service::config::RuntimeConfig;
use crate::service::service_exporter::ServiceExporter;
use crate::service::service_runtime::InitError;

pub fn initialize(config: RuntimeConfig, monitor: Option<Arc<dyn Monitor>>,) -> Result<ServiceExporter, InitError> {
    service::service_exporter::create_service_exporter(config, monitor)
}

pub fn init_default() -> Result<ServiceExporter, InitError> {
    let config = RuntimeConfig::default();
    initialize(config, None)
}

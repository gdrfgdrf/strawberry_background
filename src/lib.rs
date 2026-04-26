pub mod adapters;
pub mod domain;
pub mod infrastructure;
pub mod monitor;
pub mod rkv;
pub mod rkyv;
pub mod service;
pub mod superstructure;
pub mod utils;

use crate::service::config::RuntimeConfig;
use crate::service::service_exporter::ServiceExporter;
use crate::service::service_runtime::InitError;
use std::sync::Arc;
use tokio::runtime::Runtime;

pub fn initialize(
    config: RuntimeConfig,
    tokio_runtime: Arc<Runtime>,
) -> Result<ServiceExporter, InitError> {
    service::service_exporter::create_service_exporter_with_tokio_runtime(config, tokio_runtime)
}

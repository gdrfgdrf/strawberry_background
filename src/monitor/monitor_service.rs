use crate::domain::models::monitor_models::{MonitorError, MonitorEvent};
use crate::domain::traits::monitor_traits::{Monitor, MonitorSubscriber};
use crate::infrastructure::monitor::mpsc_monitor_backend::MpscMonitorBackend;
use std::cell::OnceCell;
use std::sync::{Arc, RwLock};
use tokio::runtime::Runtime;

pub static MONITOR_SERVICE: RwLock<Option<MonitorService>> = RwLock::new(None);

pub fn initialize_monitor(tokio_runtime: Arc<Runtime>) {
    let guard = MONITOR_SERVICE.write();
    if guard.is_err() {
        return;
    }
    let mut guard = guard.unwrap();
    if guard.is_some() {
        return;
    }
    let instance = MonitorService::new(tokio_runtime);
    *guard = Some(instance);
}

pub fn monitoring<F>(func: F)
where
    F: FnOnce(Arc<dyn Monitor>),
{
    let service = MONITOR_SERVICE.read();
    if service.is_err() {
        return;
    }
    let service = service.unwrap();
    if service.is_none() {
        return;
    }
    let service = service.as_ref().unwrap();
    let monitor = service.monitor.clone();
    func(monitor);
}

pub fn subscribe(
    func: Box<dyn Fn(Arc<MonitorEvent>) + Send + Sync>,
) -> Result<Arc<dyn MonitorSubscriber>, MonitorError> {
    let service = MONITOR_SERVICE.read();
    if service.is_err() {
        return Err(MonitorError::NotConfigured);
    }
    let service = service.unwrap();
    if service.is_none() {
        return Err(MonitorError::NotConfigured);
    }
    let service = service.as_ref().unwrap();
    let subscriber = service.monitor.subscribe(func)?;
    Ok(subscriber)
}

pub struct MonitorService {
    monitor: Arc<dyn Monitor>,
}

unsafe impl Sync for MonitorService {}
unsafe impl Send for MonitorService {}

impl MonitorService {
    pub fn new(tokio_runtime: Arc<Runtime>) -> Self {
        Self {
            monitor: MpscMonitorBackend::new(tokio_runtime),
        }
    }
}

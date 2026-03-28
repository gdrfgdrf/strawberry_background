use crate::domain::models::monitor_models::{MonitorError, MonitorEvent};
use std::sync::Arc;

pub trait Monitor {
    fn send(&self, event: MonitorEvent);
    fn subscribe(
        &self,
        callback: Box<dyn Fn(Arc<MonitorEvent>) + Send + Sync>,
    ) -> Result<Arc<dyn MonitorSubscriber>, MonitorError>;
}

pub trait MonitorSubscriber: Send + Sync {
    fn cancel(&self);
}

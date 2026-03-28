use std::sync::Arc;
use crate::domain::models::monitor_models::{MonitorError, MonitorEvent};

pub trait Monitor {
    fn send(&self, event: MonitorEvent);
    fn subscribe(&self, callback: Box<dyn Fn(Arc<MonitorEvent>)>) -> Result<Arc<dyn MonitorSubscriber>, MonitorError>;
}

pub trait MonitorSubscriber {
    fn cancel(&self);
}

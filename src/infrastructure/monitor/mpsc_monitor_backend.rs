use crate::domain::models::monitor_models::{MonitorError, MonitorEvent};
use crate::domain::traits::monitor_traits::{Monitor, MonitorSubscriber};
use dashmap::DashMap;
use std::ops::Deref;
use std::sync::{Arc, Mutex, Weak};
use tokio::runtime::Runtime;
use tokio::sync::mpsc::{Receiver, Sender};
use uuid::Uuid;

pub struct MpscMonitorBackend {
    tokio_runtime: Arc<Runtime>,
    self_weak: Mutex<Weak<MpscMonitorBackend>>,
    sender: Sender<MonitorEvent>,
    receiver: Receiver<MonitorEvent>,
    subscribers: DashMap<String, Arc<MpscMonitorSubscriber>>,
}

pub struct MpscMonitorSubscriber {
    id: String,
    monitor: Arc<MpscMonitorBackend>,
    callback: Box<dyn Fn(Arc<MonitorEvent>)>,
}

impl MpscMonitorBackend {
    pub fn new(tokio_runtime: Arc<Runtime>) -> Arc<Self> {
        let (sender, receiver) = tokio::sync::mpsc::channel::<MonitorEvent>(500);
        let monitor = Arc::new(Self {
            tokio_runtime,
            self_weak: Mutex::new(Weak::new()),
            sender,
            receiver,
            subscribers: DashMap::new(),
        });
        *monitor.self_weak.lock().unwrap() = Arc::downgrade(&monitor);
        monitor
    }

    pub fn cancel_subscriber(&self, id: &str) {
        self.subscribers.remove(id);
    }
}

impl Monitor for MpscMonitorBackend {
    fn send(&self, event: MonitorEvent) {
        let arc = Arc::new(event);
        let subscribers = &self.subscribers;
        subscribers.iter().for_each(|subscriber| {
            subscriber.notify(arc.clone());
        });
    }

    fn subscribe(
        &self,
        callback: Box<dyn Fn(Arc<MonitorEvent>)>,
    ) -> Result<Arc<dyn MonitorSubscriber>, MonitorError> {
        let self_arc = self.self_weak.lock().unwrap().clone().upgrade();
        if self_arc.is_none() {
            return Err(MonitorError::UpgradeReference(
                "monitor must be alive".to_string(),
            ));
        }
        let self_arc = self_arc.unwrap();

        let id = Uuid::new_v4().to_string();
        let subscriber = Arc::new(MpscMonitorSubscriber {
            id: id.to_string(),
            monitor: self_arc,
            callback,
        });
        self.subscribers.insert(id, subscriber.clone());

        Ok(subscriber)
    }
}

impl MpscMonitorSubscriber {
    fn notify(&self, event: Arc<MonitorEvent>) {
        self.callback.deref()(event);
    }
}

impl MonitorSubscriber for MpscMonitorSubscriber {
    fn cancel(&self) {
        self.monitor.cancel_subscriber(&self.id)
    }
}

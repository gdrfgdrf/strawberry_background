
#[derive(Clone)]
pub enum EventStage {
    Started,
    Running,
    Finished,
    Failed,
}

#[derive(Clone)]
pub struct Progress {
    pub value: u64,
    pub total: u64,
    pub delta: u64
}

#[derive(Clone)]
pub enum MonitorEvent {
    Http {
        stage: EventStage,
        url: String,
        data: Option<MonitorHttpData>
    },
    Storage {
        stage: EventStage,
        path: String,
        data: Option<MonitorStorageData>
    }
}

#[derive(Clone)]
pub struct MonitorHttpData {
    pub progress: Progress,
}

#[derive(Clone)]
pub struct MonitorStorageData {
    pub progress: Progress
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum MonitorError {
    #[error("upgrade reference error: {0}")]
    UpgradeReference(String),
    #[error("not configured")]
    NotConfigured
}

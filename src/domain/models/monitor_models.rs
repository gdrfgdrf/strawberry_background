pub enum EventStage {
    Started,
    Running,
    Finished,
    Failed,
}

pub struct Progress {
    pub value: u64,
    pub total: u64,
    pub delta: u64
}

pub enum MonitorEvent {
    Http {
        stage: EventStage,
        url: String,
        data: Option<MonitorHttpData>
    },
}

pub struct MonitorHttpData {
    pub progress: Progress,
}

#[derive(Debug, thiserror::Error)]
pub enum MonitorError {
    #[error("upgrade reference error: {0}")]
    UpgradeReference(String),
}

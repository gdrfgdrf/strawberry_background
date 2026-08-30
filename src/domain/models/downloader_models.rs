use std::cmp::Ordering;
use std::sync::Arc;
use parking_lot::RwLock;

#[derive(Debug, thiserror::Error)]
pub enum DownloaderError {
    #[error("Error Forward: {0}")]
    ErrorForward(String),
    #[error("Channel Not Exists")]
    ChannelNotExists

}

#[derive(Eq, PartialEq)]
pub enum Priority {
    Normal,
    Privileged,
}

#[derive(Eq, PartialEq)]
pub enum RequestStatus {
    Enqueued,
    Running,
    Canceled,
    Finished,
    Error {
        message: String
    },
}

pub struct DownloadRequest {
    pub id: String,
    pub url: String,
    pub path: String,
    pub priority: Arc<RwLock<Priority>>,
    pub create_time: u64,
}

pub struct Progress {
    pub length: u64,
    pub expected_length: u64,
    pub speed: f32,
}

pub struct RequestSnapshot {
    pub progress: Progress,
    pub status: RequestStatus
}

impl Priority {
    fn ordinal(&self) -> u8 {
        match self {
            Priority::Normal => 1,
            Priority::Privileged => 0,
        }
    }
}

impl RequestStatus {
    pub fn is_final(&self) -> bool {
        match self {
            RequestStatus::Enqueued => false,
            RequestStatus::Running => false,
            RequestStatus::Canceled => true,
            RequestStatus::Finished => true,
            RequestStatus::Error { .. } => true
        }
    }
}

impl Default for Progress {
    fn default() -> Self {
        Self {
            length: 0,
            expected_length: u64::MAX,
            speed: 0.0
        }
    }
}

impl Default for RequestSnapshot {
    fn default() -> Self {
        RequestSnapshot {
            progress: Progress::default(),
            status: RequestStatus::Enqueued
        }
    }
}

impl Ord for DownloadRequest {
    fn cmp(&self, other: &Self) -> Ordering {
        let p1 = self.priority.read();
        let p2 = other.priority.read();
        if *p1 != *p2 {
            return p1.ordinal().cmp(&p2.ordinal());
        }
        drop(p1);
        drop(p2);

        self.create_time.cmp(&other.create_time)
    }
}

impl PartialOrd for DownloadRequest {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Eq for DownloadRequest {}

impl PartialEq for DownloadRequest {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
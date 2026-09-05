use crate::db::models::downloader_records::DownloaderPriority;
use parking_lot::RwLock;
use std::cmp::Ordering;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum DownloaderError {
    #[error("Error Forward: {0}")]
    ErrorForward(String),
    #[error("Channel Not Exists")]
    ChannelNotExists,
}

#[derive(Debug, Eq, PartialEq)]
pub enum Priority {
    Normal,
    Privileged,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum RequestStatus {
    Enqueued,
    Running,
    Paused,
    Resumed,
    WaitForRetry { retry_at: u64 },
    Canceled,
    Finished,
    Error { message: String },
}

#[derive(Debug, Clone)]
pub struct ResolvedMeta {
    pub url: String,
    pub path: String,
}

#[derive(Debug, thiserror::Error)]
pub enum MetaError {
    #[error("{0}")]
    Retryable(String),
    #[error("{0}")]
    Fatal(String),
}

pub trait MetaProvider: Send + Sync {
    fn resolve(&self) -> Pin<Box<dyn Future<Output = Result<ResolvedMeta, MetaError>> + Send>>;
}

#[derive(Clone)]
pub enum RequestSource {
    Prepared { url: String, path: String },
    Deferred(Arc<dyn MetaProvider>),
}

impl RequestSource {
    pub fn known_url(&self) -> String {
        match self {
            RequestSource::Prepared { url, .. } => url.clone(),
            RequestSource::Deferred(_) => String::new(),
        }
    }

    pub fn known_path(&self) -> String {
        match self {
            RequestSource::Prepared { path, .. } => path.clone(),
            RequestSource::Deferred(_) => String::new(),
        }
    }
}

pub struct DownloadRequest {
    pub id: String,
    pub source: RequestSource,
    pub priority: Arc<RwLock<Priority>>,
    pub create_time: u64,
    pub channel_id: u32,
    pub resume_from: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct Progress {
    pub length: u64,
    pub expected_length: u64,
    pub speed: f32,
}

#[derive(Debug, Clone)]
pub struct RequestSnapshot {
    pub progress: Progress,
    pub status: RequestStatus,
    pub retried_count: u32,
}

impl Priority {
    fn ordinal(&self) -> u8 {
        match self {
            Priority::Normal => 1,
            Priority::Privileged => 0,
        }
    }

    pub fn as_db_priority(&self) -> DownloaderPriority {
        match self {
            Priority::Normal => DownloaderPriority::Normal,
            Priority::Privileged => DownloaderPriority::Privileged,
        }
    }

    pub fn from_db_priority(value: DownloaderPriority) -> Self {
        match value {
            DownloaderPriority::Normal => Priority::Normal,
            DownloaderPriority::Privileged => Priority::Privileged,
        }
    }
}

impl RequestStatus {
    pub fn is_final(&self) -> bool {
        match self {
            RequestStatus::Enqueued => false,
            RequestStatus::Running => false,
            RequestStatus::Paused => false,
            RequestStatus::Resumed => false,
            RequestStatus::WaitForRetry { .. } => false,
            RequestStatus::Canceled => true,
            RequestStatus::Finished => true,
            RequestStatus::Error { .. } => true,
        }
    }
}

impl Default for Progress {
    fn default() -> Self {
        Self {
            length: 0,
            expected_length: u64::MAX,
            speed: 0.0,
        }
    }
}

impl Default for RequestSnapshot {
    fn default() -> Self {
        RequestSnapshot {
            progress: Progress::default(),
            status: RequestStatus::Enqueued,
            retried_count: 0,
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

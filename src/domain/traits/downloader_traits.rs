use crate::domain::models::downloader_models::{DownloadRequest, DownloaderError};
use crate::superstructure::downloader::downloader::Output;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub struct RecoveredDownload {
    pub channel_id: u32,
    pub request_id: String,
    pub output: Arc<Output>,
    pub cancellation_token: CancellationToken,
}

pub trait Downloader {
    async fn init(&self, channel_ids: Vec<u32>) -> Result<(), DownloaderError>;
    async fn submit(
        &self,
        channel_id: u32,
        request: DownloadRequest,
    ) -> Result<(Arc<Output>, CancellationToken), DownloaderError>;
    async fn submit_batch(
        &self,
        channel_id: u32,
        requests: Vec<DownloadRequest>,
    ) -> Result<Vec<(Arc<Output>, CancellationToken)>, DownloaderError>;
    async fn recover(&self) -> Result<Vec<RecoveredDownload>, DownloaderError>;
}

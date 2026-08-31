use crate::db::models::downloader_records::DownloaderStatus;
use crate::db::models::preclude::DownloaderRecordsModel;
use crate::db::services::downloader_service::{DownloaderService, NewDownloaderRecord};
use crate::domain::models::downloader_models::{
    DownloadRequest, DownloaderError, Priority, Progress, RequestSnapshot, RequestStatus,
};
use crate::domain::models::http_models::{HttpEndpoint, HttpMethod};
use crate::domain::traits::downloader_traits::{Downloader, RecoveredDownload};
use crate::service::service_runtime::ServiceRuntime;
use crate::utils::async_priority_queue::AsyncPriorityQueue;
use crate::utils::speed_analyzer::SpeedAnalyzer;
use async_ringbuf::traits::*;
use async_ringbuf::{AsyncHeapProd, AsyncHeapRb};
use dashmap::DashMap;
use futures_util::{FutureExt, StreamExt};
use parking_lot::{Mutex, RwLock};
use std::cmp::Ordering;
use std::io::SeekFrom;
use std::ops::Deref;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering as AtomicOrdering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::fs::OpenOptions;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::runtime::Runtime;
use tokio::sync::{Notify, Semaphore, watch};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

const MAX_CONCURRENT_DOWNLOADS: usize = 6;
const MAX_RETRY_ATTEMPTS: u32 = 5;
const RETRY_BASE_DELAY: Duration = Duration::from_millis(500);
const RETRY_MAX_DELAY: Duration = Duration::from_secs(30);
const HTTP_REQUEST_TIMEOUT: Duration = Duration::from_secs(3600);
const ENQUEUE_TIMEOUT: Duration = Duration::from_secs(60);
const WRITE_BUFFER_CAPACITY: usize = 1024 * 1024;
const WRITE_DRAIN_CHUNK: usize = 64 * 1024;
const PROGRESS_PERSIST_INTERVAL_MS: u64 = 2000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum RunSignal {
    Running,
    Paused,
}

pub struct Output {
    pub snapshot_sender: watch::Sender<RequestSnapshot>,
    run_signal: watch::Sender<RunSignal>,
    retry_notify: Notify,
}

impl Output {
    fn new() -> Self {
        Self::new_with_initial(false)
    }

    fn new_with_initial(paused: bool) -> Self {
        let initial_signal = if paused {
            RunSignal::Paused
        } else {
            RunSignal::Running
        };
        Self {
            snapshot_sender: watch::channel(RequestSnapshot::default()).0,
            run_signal: watch::channel(initial_signal).0,
            retry_notify: Notify::new(),
        }
    }

    pub fn subscribe(&self) -> watch::Receiver<RequestSnapshot> {
        self.snapshot_sender.subscribe()
    }

    pub fn pause(&self) {
        let _ = self.run_signal.send(RunSignal::Paused);
    }

    pub fn resume(&self) {
        let _ = self.run_signal.send(RunSignal::Running);
    }

    pub fn retry_now(&self) {
        self.retry_notify.notify_one();
    }

    fn emit(&self, status: RequestStatus, progress: Progress, retried_count: u32) {
        let _ = self.snapshot_sender.send(RequestSnapshot {
            progress,
            status,
            retried_count,
        });
    }
}

struct AttemptState {
    downloaded: AtomicU64,
    expected_length: AtomicU64,
    retried_count: AtomicU32,
    last_persist_ms: AtomicU64,
}

impl AttemptState {
    fn new(initial_downloaded: u64, initial_retried_count: u32) -> Self {
        Self {
            downloaded: AtomicU64::new(initial_downloaded),
            expected_length: AtomicU64::new(u64::MAX),
            retried_count: AtomicU32::new(initial_retried_count),
            last_persist_ms: AtomicU64::new(0),
        }
    }

    fn downloaded(&self) -> u64 {
        self.downloaded.load(AtomicOrdering::Relaxed)
    }

    fn set_downloaded(&self, value: u64) {
        self.downloaded.store(value, AtomicOrdering::Relaxed);
    }

    fn reset_downloaded(&self) {
        self.downloaded.store(0, AtomicOrdering::Relaxed);
    }

    fn expected_length(&self) -> u64 {
        self.expected_length.load(AtomicOrdering::Relaxed)
    }

    fn set_expected_length(&self, value: u64) {
        self.expected_length.store(value, AtomicOrdering::Relaxed);
    }

    fn retried_count(&self) -> u32 {
        self.retried_count.load(AtomicOrdering::Relaxed)
    }

    fn increment_retry(&self) -> u32 {
        self.retried_count.fetch_add(1, AtomicOrdering::Relaxed) + 1
    }

    fn progress(&self) -> Progress {
        Progress {
            length: self.downloaded(),
            expected_length: self.expected_length(),
            speed: 0.0,
        }
    }

    fn should_persist_progress(&self, now_ms: u64) -> bool {
        let last = self.last_persist_ms.load(AtomicOrdering::Relaxed);
        if now_ms.saturating_sub(last) >= PROGRESS_PERSIST_INTERVAL_MS {
            self.last_persist_ms.store(now_ms, AtomicOrdering::Relaxed);
            true
        } else {
            false
        }
    }
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn exponential_backoff(retry_count: u32) -> Duration {
    let exponent = retry_count.saturating_sub(1).min(10);
    let millis = RETRY_BASE_DELAY
        .as_millis()
        .saturating_mul(1u128 << exponent)
        .min(RETRY_MAX_DELAY.as_millis());
    Duration::from_millis(millis as u64)
}

fn persist_progress(request_id: &str, downloaded: u64, expected_length: u64) {
    let request_id = request_id.to_string();
    let downloaded = downloaded as i64;
    let expected_length = if expected_length == u64::MAX {
        None
    } else {
        Some(expected_length as i64)
    };
    tokio::spawn(async move {
        let _ = DownloaderService::update_progress(&request_id, downloaded, expected_length).await;
    });
}

fn persist_status(
    request_id: &str,
    status: DownloaderStatus,
    downloaded: u64,
    retried_count: u32,
    error_message: Option<String>,
) {
    let request_id = request_id.to_string();
    tokio::spawn(async move {
        let _ = DownloaderService::update_status(
            &request_id,
            status,
            downloaded as i64,
            retried_count as i32,
            error_message,
        )
        .await;
    });
}

fn persist_removal(request_id: &str) {
    let request_id = request_id.to_string();
    tokio::spawn(async move {
        let _ = DownloaderService::remove_by_request_id(&request_id).await;
    });
}

pub struct WrappedRequest {
    pub request: DownloadRequest,
    pub output: Arc<Output>,
    pub cancellation_token: CancellationToken,
    pub initial_retried_count: u32,
}

impl WrappedRequest {
    pub fn new(
        request: DownloadRequest,
        output: Arc<Output>,
        cancellation_token: CancellationToken,
    ) -> Self {
        Self::with_retried_count(request, output, cancellation_token, 0)
    }

    pub fn with_retried_count(
        request: DownloadRequest,
        output: Arc<Output>,
        cancellation_token: CancellationToken,
        initial_retried_count: u32,
    ) -> Self {
        Self {
            request,
            output,
            cancellation_token,
            initial_retried_count,
        }
    }
}

pub struct HttpDownloader {
    runtime: Arc<Runtime>,
    service_runtime: Arc<ServiceRuntime>,
    channels: DashMap<u32, Arc<DownloaderChannel>>,
}

pub struct DownloaderChannel {
    runtime: Arc<Runtime>,
    service_runtime: Arc<ServiceRuntime>,
    queue: Arc<AsyncPriorityQueue<WrappedRequest>>,
    tasks: Arc<Mutex<JoinSet<()>>>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl HttpDownloader {
    pub fn new(runtime: Arc<Runtime>, service_runtime: Arc<ServiceRuntime>) -> Self {
        Self {
            runtime,
            service_runtime,
            channels: DashMap::with_capacity(3),
        }
    }
}

impl DownloaderChannel {
    pub fn new(runtime: Arc<Runtime>, service_runtime: Arc<ServiceRuntime>) -> Self {
        Self {
            runtime,
            service_runtime,
            queue: Arc::new(AsyncPriorityQueue::unbounded()),
            tasks: Arc::new(Mutex::new(JoinSet::new())),
            handle: Mutex::new(None),
        }
    }

    pub fn init(&self) -> Result<(), DownloaderError> {
        let queue = self.queue.clone();
        let tasks = self.tasks.clone();
        let service_runtime = self.service_runtime.clone();

        *self.handle.lock() = Some(self.runtime.spawn(async move {
            let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_DOWNLOADS));

            loop {
                let wrapped = queue.pop().await;
                let semaphore = semaphore.clone();
                let service_runtime = service_runtime.clone();
                let mut tasks = tasks.lock();
                while tasks.try_join_next().is_some() {}
                tasks.spawn(run_request(wrapped, semaphore, service_runtime));
            }
        }));

        Ok(())
    }

    pub async fn submit(
        &self,
        request: DownloadRequest,
    ) -> Result<(Arc<Output>, CancellationToken), DownloaderError> {
        let new_record = NewDownloaderRecord {
            request_id: request.id.clone(),
            channel_id: request.channel_id as i64,
            url: request.url.clone(),
            path: request.path.clone(),
            priority: request.priority.read().as_db_priority(),
            downloaded: request.resume_from as i64,
            status: DownloaderStatus::Enqueued,
        };
        let _ = DownloaderService::upsert(new_record).await;
        let output = Arc::new(Output::new());
        let cancellation_token = CancellationToken::new();
        let wrapped = WrappedRequest::new(request, output.clone(), cancellation_token.clone());
        timeout(ENQUEUE_TIMEOUT, self.queue.push(wrapped)).await?;
        Ok((output, cancellation_token))
    }

    pub async fn submit_recovered(
        &self,
        record: DownloaderRecordsModel,
    ) -> Result<(Arc<Output>, CancellationToken), DownloaderError> {
        let resume_from = tokio::fs::metadata(&record.path)
            .await
            .map(|meta| meta.len())
            .unwrap_or(0);
        let initial_paused = record.status == DownloaderStatus::Paused;
        let initial_retried_count = record.retried_count.max(0) as u32;
        let create_time = record.created_at.timestamp_millis().max(0) as u64;

        let request = DownloadRequest {
            id: record.request_id,
            url: record.url,
            path: record.path,
            priority: Arc::new(RwLock::new(Priority::from_db_priority(record.priority))),
            create_time,
            channel_id: record.channel_id as u32,
            resume_from,
        };

        let _ = DownloaderService::update_status(
            &request.id,
            if initial_paused {
                DownloaderStatus::Paused
            } else {
                DownloaderStatus::Running
            },
            resume_from as i64,
            initial_retried_count as i32,
            None,
        )
        .await;

        let output = Arc::new(Output::new_with_initial(initial_paused));
        let cancellation_token = CancellationToken::new();
        let wrapped = WrappedRequest::with_retried_count(
            request,
            output.clone(),
            cancellation_token.clone(),
            initial_retried_count,
        );
        timeout(ENQUEUE_TIMEOUT, self.queue.push(wrapped)).await?;
        Ok((output, cancellation_token))
    }
}

impl Downloader for HttpDownloader {
    async fn init(&self, channel_ids: Vec<u32>) -> Result<(), DownloaderError> {
        for channel_id in channel_ids.into_iter() {
            let channel =
                DownloaderChannel::new(self.runtime.clone(), self.service_runtime.clone());
            channel.init()?;
            self.channels.insert(channel_id, Arc::new(channel));
        }

        Ok(())
    }

    async fn submit(
        &self,
        channel_id: u32,
        mut request: DownloadRequest,
    ) -> Result<(Arc<Output>, CancellationToken), DownloaderError> {
        let channel = self.channels.get(&channel_id).map(|entry| entry.clone());
        let Some(channel) = channel else {
            return Err(DownloaderError::ChannelNotExists);
        };
        request.channel_id = channel_id;
        channel.submit(request).await
    }

    async fn recover(&self) -> Result<Vec<RecoveredDownload>, DownloaderError> {
        let records = DownloaderService::find_resumable().await?;

        let mut recovered = Vec::with_capacity(records.len());
        for record in records {
            let channel_id = record.channel_id as u32;
            let request_id = record.request_id.clone();
            let channel = self.channels.get(&channel_id).map(|entry| entry.clone());
            let Some(channel) = channel else {
                continue;
            };

            match channel.submit_recovered(record).await {
                Ok((output, cancellation_token)) => recovered.push(RecoveredDownload {
                    channel_id,
                    request_id,
                    output,
                    cancellation_token,
                }),
                Err(_) => {}
            }
        }

        Ok(recovered)
    }
}

impl Deref for WrappedRequest {
    type Target = DownloadRequest;

    fn deref(&self) -> &Self::Target {
        &self.request
    }
}

impl Ord for WrappedRequest {
    fn cmp(&self, other: &Self) -> Ordering {
        self.request.cmp(&other.request)
    }
}

impl PartialOrd for WrappedRequest {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Eq for WrappedRequest {}

impl PartialEq for WrappedRequest {
    fn eq(&self, other: &Self) -> bool {
        self.request.eq(&other.request)
    }
}

enum AttemptOutcome {
    Finished,
    Paused,
    Failed(String),
}

async fn run_request(
    wrapped: WrappedRequest,
    semaphore: Arc<Semaphore>,
    service_runtime: Arc<ServiceRuntime>,
) {
    let WrappedRequest {
        request,
        output,
        cancellation_token,
        initial_retried_count,
    } = wrapped;
    let state = AttemptState::new(request.resume_from, initial_retried_count);

    let attempt = attempt_download(&request, &output, &semaphore, &service_runtime, &state);
    if cancellation_token
        .run_until_cancelled(attempt)
        .await
        .is_none()
    {
        output.emit(
            RequestStatus::Canceled,
            state.progress(),
            state.retried_count(),
        );
        persist_removal(&request.id);
    }
}

async fn attempt_download(
    request: &DownloadRequest,
    output: &Arc<Output>,
    semaphore: &Arc<Semaphore>,
    service_runtime: &Arc<ServiceRuntime>,
    state: &AttemptState,
) {
    let mut run_signal_receiver = output.run_signal.subscribe();

    loop {
        if matches!(*run_signal_receiver.borrow(), RunSignal::Paused) {
            output.emit(
                RequestStatus::Paused,
                state.progress(),
                state.retried_count(),
            );
            persist_status(
                &request.id,
                DownloaderStatus::Paused,
                state.downloaded(),
                state.retried_count(),
                None,
            );

            loop {
                if run_signal_receiver.changed().await.is_err() {
                    return;
                }
                if matches!(*run_signal_receiver.borrow(), RunSignal::Running) {
                    break;
                }
            }

            output.emit(
                RequestStatus::Resumed,
                state.progress(),
                state.retried_count(),
            );
        }

        let permit = match semaphore.acquire().await {
            Ok(permit) => permit,
            Err(_) => return,
        };
        persist_status(
            &request.id,
            DownloaderStatus::Running,
            state.downloaded(),
            state.retried_count(),
            None,
        );

        let outcome =
            run_single_attempt(request, output, service_runtime, state, &mut run_signal_receiver).await;
        drop(permit);

        match outcome {
            AttemptOutcome::Finished => {
                output.emit(
                    RequestStatus::Finished,
                    state.progress(),
                    state.retried_count(),
                );
                persist_removal(&request.id);
                return;
            }
            AttemptOutcome::Paused => continue,
            AttemptOutcome::Failed(message) => {
                if state.retried_count() >= MAX_RETRY_ATTEMPTS {
                    output.emit(
                        RequestStatus::Error {
                            message: message.clone(),
                        },
                        state.progress(),
                        state.retried_count(),
                    );
                    persist_status(
                        &request.id,
                        DownloaderStatus::Error,
                        state.downloaded(),
                        state.retried_count(),
                        Some(message),
                    );
                    return;
                }

                let retried_count = state.increment_retry();
                let delay = exponential_backoff(retried_count);
                let retry_at = now_unix_ms() + delay.as_millis() as u64;
                output.emit(
                    RequestStatus::WaitForRetry { retry_at },
                    state.progress(),
                    retried_count,
                );
                persist_status(
                    &request.id,
                    DownloaderStatus::WaitForRetry,
                    state.downloaded(),
                    retried_count,
                    None,
                );

                let _ = output.retry_notify.notified().now_or_never();

                tokio::select! {
                    _ = tokio::time::sleep(delay) => {},
                    _ = output.retry_notify.notified() => {},
                    _ = run_signal_receiver.changed() => {},
                }
            }
        }
    }
}

async fn run_single_attempt(
    request: &DownloadRequest,
    output: &Arc<Output>,
    service_runtime: &Arc<ServiceRuntime>,
    state: &AttemptState,
    run_signal_receiver: &mut watch::Receiver<RunSignal>,
) -> AttemptOutcome {
    let offset = state.downloaded();
    let headers = if offset > 0 {
        Some(vec![("Range".to_string(), format!("bytes={}-", offset))])
    } else {
        None
    };

    let endpoint = HttpEndpoint {
        path: String::new(),
        domain: request.url.clone(),
        body: None,
        timeout: HTTP_REQUEST_TIMEOUT,
        headers,
        path_params: None,
        query_params: None,
        method: HttpMethod::Get,
        requires_encryption: false,
        requires_decryption: false,
        user_agent: None,
        content_type: None,
    };

    let response = match service_runtime.execute_stream_http(endpoint).await {
        Ok(Ok(response)) => response,
        Ok(Err(http_err)) => return AttemptOutcome::Failed(http_err.to_string()),
        Err(service_err) => return AttemptOutcome::Failed(service_err.to_string()),
    };

    let restart_from_scratch = offset > 0 && response.status != 206;
    let start_offset = if restart_from_scratch { 0 } else { offset };
    if restart_from_scratch {
        state.reset_downloaded();
    }

    let mut file = match StreamFileWriter::open(&request.path, start_offset).await {
        Ok(file) => file,
        Err(err) => return AttemptOutcome::Failed(format!("failed to open file: {err}")),
    };

    let body_length = response.content_length.unwrap_or(u64::MAX);
    let total_expected = if body_length == u64::MAX {
        u64::MAX
    } else {
        body_length.saturating_add(start_offset)
    };
    state.set_expected_length(total_expected);

    let mut speed_analyzer = SpeedAnalyzer::new();
    speed_analyzer.start();

    let mut attempt_received: u64 = 0;

    let mut stream = response.stream;
    loop {
        tokio::select! {
            biased;

            changed = run_signal_receiver.changed() => {
                if changed.is_err() {
                    return AttemptOutcome::Failed("output was dropped".to_string());
                }
                if matches!(*run_signal_receiver.borrow(), RunSignal::Paused) {
                    let (written, result) = file.finish().await;
                    state.set_downloaded(written);
                    if let Err(err) = result {
                        return AttemptOutcome::Failed(format!("disk write failed: {err}"));
                    }
                    return AttemptOutcome::Paused;
                }
            }

            chunk = stream.next() => {
                match chunk {
                    None => {
                        let (written, result) = file.finish().await;
                        state.set_downloaded(written);
                        return match result {
                            Ok(()) => AttemptOutcome::Finished,
                            Err(err) => AttemptOutcome::Failed(format!("disk write failed: {err}")),
                        };
                    }
                    Some(Err(err)) => {
                        let (written, _result) = file.finish().await;
                        state.set_downloaded(written);
                        return AttemptOutcome::Failed(err.to_string());
                    }
                    Some(Ok(bytes)) => {
                        let length = bytes.len() as u64;
                        if file.write_chunk(&bytes).await.is_err() {
                            let (written, result) = file.finish().await;
                            state.set_downloaded(written);
                            let message = match result {
                                Err(err) => format!("disk write failed: {err}"),
                                Ok(()) => "disk writer task ended unexpectedly".to_string(),
                            };
                            return AttemptOutcome::Failed(message);
                        }

                        attempt_received += length;
                        speed_analyzer.add(length);

                        let live_downloaded = start_offset + attempt_received;
                        let progress = Progress {
                            length: live_downloaded,
                            expected_length: state.expected_length(),
                            speed: speed_analyzer.speed(),
                        };
                        output.emit(RequestStatus::Running, progress, state.retried_count());

                        if state.should_persist_progress(now_unix_ms()) {
                            persist_progress(&request.id, live_downloaded, state.expected_length());
                        }
                    }
                }
            }
        }
    }
}

struct StreamFileWriter {
    producer: AsyncHeapProd<u8>,
    written: Arc<AtomicU64>,
    task: JoinHandle<std::io::Result<()>>,
}

impl StreamFileWriter {
    async fn open(path: &str, offset: u64) -> std::io::Result<Self> {
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .open(path)
            .await?;
        file.set_len(offset).await?;
        file.seek(SeekFrom::Start(offset)).await?;

        let ring = AsyncHeapRb::<u8>::new(WRITE_BUFFER_CAPACITY);
        let (producer, mut consumer) = ring.split();
        let written = Arc::new(AtomicU64::new(0));
        let written_for_task = written.clone();

        let task = tokio::spawn(async move {
            let mut buf = vec![0u8; WRITE_DRAIN_CHUNK];
            loop {
                consumer.wait_occupied(1).await;
                let popped = consumer.pop_slice(&mut buf);
                if popped == 0 {
                    break;
                }
                file.write_all(&buf[..popped]).await?;
                written_for_task.fetch_add(popped as u64, AtomicOrdering::Relaxed);
            }
            file.sync_all().await
        });

        Ok(Self {
            producer,
            written,
            task,
        })
    }

    async fn write_chunk(&mut self, bytes: &[u8]) -> Result<(), ()> {
        self.producer.push_exact(bytes).await.map_err(|_| ())
    }

    async fn finish(self) -> (u64, std::io::Result<()>) {
        let Self {
            mut producer,
            written,
            task,
        } = self;
        producer.close();
        let result = match task.await {
            Ok(result) => result,
            Err(join_err) => Err(std::io::Error::other(join_err.to_string())),
        };
        (written.load(AtomicOrdering::Relaxed), result)
    }
}

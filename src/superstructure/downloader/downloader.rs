use crate::domain::models::downloader_models::{
    DownloadRequest, DownloaderError, Progress, RequestSnapshot, RequestStatus,
};
use crate::domain::models::http_models::{
    HttpClientError, HttpEndpoint, HttpMethod,
};
use crate::domain::traits::downloader_traits::Downloader;
use crate::service::service_runtime::ServiceRuntime;
use crate::utils::async_priority_queue::AsyncPriorityQueue;
use crate::utils::speed_analyzer::SpeedAnalyzer;
use crate::utils::url_component::extract_domain;
use async_ringbuf::traits::{AsyncProducer, Split};
use async_ringbuf::wrap::{AsyncCons, AsyncProd};
use async_ringbuf::{AsyncHeapRb, AsyncRb};
use bytes::Bytes;
use dashmap::DashMap;
use futures_util::StreamExt;
use parking_lot::Mutex;
use ringbuf::storage::Heap;
use std::cmp::Ordering;
use std::ops::Deref;
use std::sync::Arc;
use std::time::Duration;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio::runtime::Runtime;
use tokio::sync::{Semaphore, watch};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

pub struct Output {
    pub snapshot_sender: watch::Sender<RequestSnapshot>,
}

pub struct WrappedRequest {
    pub request: DownloadRequest,
    pub writer: StreamFileWriter,
    pub output: Arc<Output>,
    pub cancellation_token: CancellationToken,
}

pub struct HttpDownloader {
    runtime: Arc<Runtime>,
    service_runtime: Arc<ServiceRuntime>,
    channels: DashMap<u32, DownloaderChannel>,
}

pub struct DownloaderChannel {
    runtime: Arc<Runtime>,
    service_runtime: Arc<ServiceRuntime>,
    queue: Arc<AsyncPriorityQueue<WrappedRequest>>,
    tasks: Arc<Mutex<JoinSet<()>>>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

pub struct StreamFileWriter {
    runtime: Arc<Runtime>,
    path: Mutex<Option<String>>,
    provider: tokio::sync::Mutex<AsyncProd<Arc<AsyncRb<Heap<Bytes>>>>>,
    consumer: Mutex<Option<AsyncCons<Arc<AsyncRb<Heap<Bytes>>>>>>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl Output {
    fn new() -> Self {
        Self {
            snapshot_sender: watch::channel(RequestSnapshot::default()).0,
        }
    }
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
        let cloned_queue = self.queue.clone();
        let cloned_tasks = self.tasks.clone();
        let cloned_service_runtime = self.service_runtime.clone();
        *self.handle.lock() = Some(self.runtime.spawn(async move {
            let queue = cloned_queue;
            let semaphore = Arc::new(Semaphore::new(6));
            let tasks = cloned_tasks;
            let service_runtime = cloned_service_runtime;
            loop {
                let request = queue.pop().await;
                let cloned_semaphore = semaphore.clone();
                let cloned_service_runtime = service_runtime.clone();
                let cloned_tasks = tasks.clone();

                let output = request.output.clone();
                let cancellation_token = &request.cancellation_token;
                cancellation_token
                    .run_until_cancelled(async move {
                        let mut tasks = cloned_tasks.lock();
                        while tasks.try_join_next().is_some() {}
                        tasks.spawn(async move {
                            let _permit = cloned_semaphore.acquire().await;

                            let inner_request = request.request;
                            let domain = extract_domain(&inner_request.url);
                            if domain.is_err() {
                                return;
                            }
                            let domain = domain.unwrap();
                            let endpoint = HttpEndpoint {
                                path: inner_request.url,
                                domain,
                                body: None,
                                timeout: Duration::from_secs(60),
                                headers: None,
                                path_params: None,
                                query_params: None,
                                method: HttpMethod::Get,
                                requires_encryption: false,
                                requires_decryption: false,
                                user_agent: None,
                                content_type: None,
                            };

                            match cloned_service_runtime.execute_stream_http(endpoint).await {
                                Ok(response) => match response {
                                    Ok(response) => {
                                        let content_length =
                                            response.content_length.unwrap_or(u64::MAX);
                                        let mut speed_analyzer = SpeedAnalyzer::new();
                                        speed_analyzer.start();

                                        let mut err = Option::<HttpClientError>::None;
                                        let writer = &request.writer;
                                        let output = request.output.clone();
                                        let mut stream = response.stream;
                                        while let Some(data) = stream.next().await {
                                            if data.is_err() {
                                                err = Some(data.unwrap_err());
                                                break;
                                            }
                                            let bytes = data.unwrap();
                                            let length = bytes.len() as u64;
                                            let _ = writer.push(bytes).await;

                                            speed_analyzer.add(length);
                                            let speed = speed_analyzer.speed();

                                            let progress = Progress {
                                                length: speed_analyzer.total,
                                                expected_length: content_length,
                                                speed,
                                            };
                                            let snapshot = RequestSnapshot {
                                                progress,
                                                status: RequestStatus::Running,
                                            };
                                            let _ = output.snapshot_sender.send(snapshot);
                                        }
                                        if err.is_none() {
                                            let snapshot = RequestSnapshot {
                                                progress: Progress::default(),
                                                status: RequestStatus::Finished,
                                            };
                                            let _ = output.snapshot_sender.send(snapshot);
                                            return;
                                        }
                                        let snapshot = RequestSnapshot {
                                            progress: Progress::default(),
                                            status: RequestStatus::Error {
                                                message: err.unwrap().to_string(),
                                            },
                                        };
                                        let _ = output.snapshot_sender.send(snapshot);
                                    }
                                    Err(_) => {}
                                },
                                Err(_) => {}
                            }
                        });
                    })
                    .await;

                if cancellation_token.is_cancelled() {
                    let snapshot = RequestSnapshot {
                        progress: Progress::default(),
                        status: RequestStatus::Canceled,
                    };
                    let _ = output.snapshot_sender.send(snapshot);
                }
            }
        }));

        Ok(())
    }

    pub async fn submit(
        &self,
        request: DownloadRequest,
    ) -> Result<(Arc<Output>, CancellationToken), DownloaderError> {
        let writer = StreamFileWriter::new(self.runtime.clone(), request.path.clone());
        let output = Arc::new(Output::new());
        let cloned_output = output.clone();
        let cancellation_token = CancellationToken::new();
        let wrapped = WrappedRequest::new(request, writer, output, cancellation_token.clone());
        timeout(Duration::from_secs(60), self.queue.push(wrapped)).await?;
        Ok((cloned_output, cancellation_token))
    }
}

impl WrappedRequest {
    pub fn new(
        request: DownloadRequest,
        writer: StreamFileWriter,
        output: Arc<Output>,
        cancellation_token: CancellationToken,
    ) -> Self {
        Self {
            request,
            writer,
            output,
            cancellation_token,
        }
    }
}

impl StreamFileWriter {
    pub fn new(runtime: Arc<Runtime>, path: String) -> Self {
        let rb = AsyncHeapRb::<Bytes>::new(256);
        let (provider, consumer) = rb.split();
        Self {
            runtime,
            path: Mutex::new(Some(path)),
            provider: tokio::sync::Mutex::new(provider),
            consumer: Mutex::new(Some(consumer)),
            handle: Mutex::new(None),
        }
    }

    pub fn init(&self) {
        let mut path_guard = self.path.lock();
        let mut consumer_guard = self.consumer.lock();
        let path = path_guard.take();
        let consumer = consumer_guard.take();
        drop(path_guard);
        drop(consumer_guard);
        if path.is_none() || consumer.is_none() {
            return;
        }

        let path = path.unwrap();
        let consumer = consumer.unwrap();
        *self.handle.lock() = Some(self.runtime.spawn(async move {
            let path = path;
            let mut consumer = consumer;
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .truncate(true)
                .open(&path)
                .await;
            if file.is_err() {
                return;
            }
            let mut file = file.unwrap();
            while let Some(data) = consumer.next().await {
                let result = file.write_all(&data).await;
                if result.is_err() {
                    break;
                }
            }
            let _ = file.sync_all().await;
        }));
    }

    pub async fn push(&self, bytes: Bytes) -> Result<(), DownloaderError> {
        let mut provider = self.provider.lock().await;
        let _ = provider.push(bytes).await;

        Ok(())
    }

    pub async fn finish(self) -> Result<(), DownloaderError> {
        let mut provider = self.provider.lock().await;
        provider.close();

        Ok(())
    }
}

impl Downloader for HttpDownloader {
    async fn init(&self, channel_ids: Vec<u32>) -> Result<(), DownloaderError> {
        for channel_id in channel_ids.into_iter() {
            let channel =
                DownloaderChannel::new(self.runtime.clone(), self.service_runtime.clone());
            channel.init()?;
            self.channels.insert(channel_id, channel);
        }

        Ok(())
    }

    async fn submit(
        &self,
        channel_id: u32,
        request: DownloadRequest,
    ) -> Result<(Arc<Output>, CancellationToken), DownloaderError> {
        let channel = self.channels.get(&channel_id);
        if channel.is_none() {
            return Err(DownloaderError::ChannelNotExists);
        }
        let channel = channel.unwrap();
        channel.submit(request).await
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
        Some(self.cmp(&other))
    }
}

impl Eq for WrappedRequest {}

impl PartialEq for WrappedRequest {
    fn eq(&self, other: &Self) -> bool {
        self.request.eq(&other.request)
    }
}

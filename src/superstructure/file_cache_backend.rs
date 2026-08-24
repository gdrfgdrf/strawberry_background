use crate::db::models::preclude::{
    CacheChannelsActiveModel, CacheChannelsModel, CacheRecordsActiveModel, CacheRecordsModel,
};
use crate::db::services::cache_service::CacheService;
use crate::domain::models::file_cache_models::CacheError;
use crate::domain::models::storage_models::{EnsureMode, WriteMode};
use crate::domain::traits::file_cache_traits::{AsyncFileCacheManager, AsyncFileOperator};
use crate::utils::async_priority_queue::AsyncPriorityQueue;
use bytes::Bytes;
use parking_lot::{Mutex, RwLock};
use sea_orm::{ActiveValue, IntoActiveModel};
use std::cmp::Ordering;
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::{Arc, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio::runtime::Runtime;
use tokio::sync::{oneshot, Semaphore};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Eq, PartialEq)]
enum Priority {
    Normal,
    Flush,
}

struct QueuedRequest {
    path: String,
    bytes: Bytes,
    write_mode: WriteMode,
    ensure_mode: Option<EnsureMode>,
    add_time: u64,
    priority: RwLock<Priority>,
    finish_sender: Mutex<Option<oneshot::Sender<()>>>,
    finish_receiver: Mutex<Option<oneshot::Receiver<()>>>,
}

pub struct FileCacheCoordinator<T: AsyncFileCacheManager> {
    managers: HashMap<String, Arc<T>>,
}

pub struct DefaultAsyncFileCacheManager {
    base_path: String,

    channel: CacheChannelsModel,
    pending_records: RwLock<HashMap<String, Weak<Semaphore>>>,
    records: Arc<RwLock<HashMap<String, CacheRecordsModel>>>,

    operator: Arc<DefaultAsyncFileOperator>,
}

pub struct DefaultAsyncFileOperator {
    tokio_runtime: Arc<Runtime>,
    queue: Arc<AsyncPriorityQueue<Arc<QueuedRequest>>>,
    stash: Arc<RwLock<HashMap<String, Weak<QueuedRequest>>>>,
    handle: Mutex<Option<JoinHandle<()>>>,
    cancellation_token: CancellationToken,
}

impl FileCacheCoordinator<DefaultAsyncFileCacheManager> {
    pub async fn new(
        tokio_runtime: Arc<Runtime>,
        base_path: String,
        channel_name2extension: Vec<(String, Option<String>)>,
    ) -> Result<Self, CacheError> {
        let channel_names = channel_name2extension
            .iter()
            .map(|pair| pair.0.clone())
            .collect::<Vec<String>>();
        let channels = CacheService::find_channels_by_names(channel_names.clone()).await?;
        if channels.is_none() {
            return Err(CacheError::ChannelNotExists(channel_names.join(", ")));
        }
        let mut channels = channels.unwrap();
        let mut pending_channels = Vec::<CacheChannelsActiveModel>::new();
        for (index, channel) in channels.iter_mut().enumerate() {
            if channel.is_some() {
                continue;
            }
            let pair = channel_name2extension.get(index);
            if pair.is_none() {
                continue;
            }
            let pair = pair.unwrap();
            let name = pair.0.clone();
            let extension = pair.1.clone();
            let active_model = CacheChannelsActiveModel {
                id: ActiveValue::NotSet,
                name: ActiveValue::Set(name),
                extension: ActiveValue::Set(extension),
            };
            pending_channels.push(active_model);
        }
        let names = pending_channels
            .iter()
            .map(|pending_channel| pending_channel.name.clone().unwrap())
            .collect::<Vec<String>>();
        CacheService::insert_channels(pending_channels).await?;
        let mut pending_channels = VecDeque::from(
            CacheService::find_channels_by_names(names)
                .await?
                .unwrap_or(Vec::new())
                .into_iter()
                .map(|pending_channel| pending_channel.unwrap())
                .collect::<Vec<CacheChannelsModel>>(),
        );

        for channel in channels.iter_mut() {
            if channel.is_some() {
                continue;
            }
            let pending_channel = pending_channels.pop_front();
            if pending_channel.is_none() {
                continue;
            }
            let pending_channel = pending_channel.unwrap();
            *channel = Some(pending_channel);
        }

        let channels = channels
            .into_iter()
            .map(|channel| channel.unwrap())
            .collect::<Vec<CacheChannelsModel>>();
        let mut managers = HashMap::<String, Arc<DefaultAsyncFileCacheManager>>::new();
        for channel in channels {
            let name = channel.name.clone();
            let manager = Arc::new(
                DefaultAsyncFileCacheManager::new(
                    tokio_runtime.clone(),
                    base_path.clone(),
                    channel,
                )
                .await?,
            );

            managers.insert(name, manager);
        }

        Ok(Self { managers })
    }

    pub fn manager(&self, channel_name: &String) -> Option<Arc<DefaultAsyncFileCacheManager>> {
        self.managers
            .get(channel_name)
            .map(|manager| manager.clone())
    }
}

impl DefaultAsyncFileCacheManager {
    pub async fn new(
        tokio_runtime: Arc<Runtime>,
        base_path: String,
        channel: CacheChannelsModel,
    ) -> Result<Self, CacheError> {
        let records = CacheService::find_records_by_channel_id(channel.id.clone())
            .await?
            .unwrap_or(Vec::new());
        let operator = Arc::new(DefaultAsyncFileOperator::new(tokio_runtime));
        operator.init();
        Ok(Self {
            base_path,
            channel,
            pending_records: RwLock::new(HashMap::new()),
            records: Arc::new(RwLock::new(HashMap::from_iter(
                records
                    .into_iter()
                    .map(|e| (e.tag.clone(), e))
                    .collect::<Vec<(String, CacheRecordsModel)>>(),
            ))),
            operator,
        })
    }

    pub fn build_file_path(&self, filename: &String) -> String {
        let extension = self.channel.extension.as_ref();
        let name = &self.channel.name;
        if extension.is_none() {
            return format!("{}/{}/{}", self.base_path, name, filename);
        }
        format!(
            "{}/{}/{}.{}",
            self.base_path,
            name,
            filename,
            extension.unwrap()
        )
    }
}

impl DefaultAsyncFileOperator {
    pub fn new(tokio_runtime: Arc<Runtime>) -> Self {
        Self {
            tokio_runtime,
            queue: Arc::new(AsyncPriorityQueue::with_capacity(128)),
            stash: Arc::new(RwLock::new(HashMap::with_capacity(128))),
            handle: Mutex::new(None),
            cancellation_token: CancellationToken::new(),
        }
    }

    async fn ensure_parent_dir_exists(path: &String) -> Result<(), CacheError> {
        if let Some(parent) = Path::new(path).parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        Ok(())
    }

    async fn write_file(bytes: &QueuedRequest) -> Result<(), CacheError> {
        let path = &bytes.path;
        Self::ensure_parent_dir_exists(path).await?;

        let mut file = OpenOptions::new()
            .create(true)
            .append(bytes.write_mode == WriteMode::Append)
            .write(bytes.write_mode == WriteMode::Cover)
            .open(&bytes.path)
            .await?;
        timeout(Duration::from_secs(60), file.write_all(&bytes.bytes)).await??;
        if bytes.ensure_mode.is_none() {
            return Ok(());
        }
        let ensure_mode = bytes.ensure_mode.as_ref().unwrap();
        Ok(match ensure_mode {
            EnsureMode::Flush => timeout(Duration::from_secs(60), file.flush()).await??,
            EnsureMode::SyncData => timeout(Duration::from_secs(60), file.sync_data()).await??,
            EnsureMode::SyncAll => timeout(Duration::from_secs(60), file.sync_all()).await??,
        })
    }

    async fn read_file(path: &String) -> Result<Vec<u8>, CacheError> {
        let bytes = tokio::fs::read(path).await?;
        Ok(bytes)
    }

    pub fn init(&self) {
        let cloned_queue = self.queue.clone();
        let cloned_stash = self.stash.clone();
        let cloned_cancellation_token = self.cancellation_token.clone();
        *self.handle.lock() = Some(self.tokio_runtime.spawn(async move {
            let queue = cloned_queue;
            let stash = cloned_stash;
            let cancellation_token = cloned_cancellation_token;
            let semaphore = Arc::new(Semaphore::new(16));

            cancellation_token
                .run_until_cancelled(async {
                    loop {
                        let request = queue.pop().await;
                        let cloned_stash = stash.clone();
                        let cloned_semaphore = semaphore.clone();
                        tokio::spawn(async move {
                            let _ = cloned_semaphore.acquire().await;
                            let _ = Self::write_file(&request).await;
                            cloned_stash.write().remove(&request.path);

                            let mut finish_sender = request.finish_sender.lock();
                            if finish_sender.is_some() {
                                let sender = finish_sender.take().unwrap();
                                drop(finish_sender);
                                let _ = sender.send(());
                            }
                        });
                    }
                })
                .await;
        }));
    }

    fn dispose(&self) {
        self.cancellation_token.cancel();
    }
}

impl AsyncFileCacheManager for DefaultAsyncFileCacheManager {
    async fn cache(&self, tag: String, sentence: String, bytes: Bytes) -> Result<(), CacheError> {
        let mut pending_records = self.pending_records.write();
        let (permit, arc_semaphore) = if let Some(weak) = pending_records.get(&tag) {
            match weak.upgrade() {
                Some(arc) => {
                    let acquire_fut = arc.clone().acquire_owned();
                    drop(pending_records);
                    let permit = timeout(Duration::from_secs(60), acquire_fut)
                        .await?
                        .map_err(|e| CacheError::ErrorForward(e.to_string()))?;
                    (permit, arc)
                }
                None => {
                    let new_arc = Arc::new(Semaphore::new(1));
                    pending_records.insert(tag.clone(), Arc::downgrade(&new_arc));
                    let acquire_future = new_arc.clone().acquire_owned();
                    drop(pending_records);
                    let permit = timeout(Duration::from_secs(60), acquire_future)
                        .await?
                        .map_err(|e| CacheError::ErrorForward(e.to_string()))?;
                    (permit, new_arc)
                }
            }
        } else {
            let arc = Arc::new(Semaphore::new(1));
            pending_records.insert(tag.clone(), Arc::downgrade(&arc));
            let acquire_future = arc.clone().acquire_owned();
            drop(pending_records);
            let permit = acquire_future
                .await
                .map_err(|e| CacheError::ErrorForward(e.to_string()))?;
            (permit, arc)
        };
        let _permit = permit;

        let mut records = self.records.write();
        if records.contains_key(&tag) {
            let record = records.remove(&tag).unwrap();
            drop(records);

            let id = record.id.clone();
            let filename = record.filename.clone();

            let record = CacheRecordsModel {
                id: record.id,
                tag: record.tag,
                filename: record.filename,
                sentence,
                channel_id: record.channel_id,
            };
            let path = self.build_file_path(&filename);
            let result = self
                .operator
                .write(
                    path.clone(),
                    bytes,
                    WriteMode::Cover,
                    Some(EnsureMode::SyncAll),
                )
                .await;
            let result = match result {
                Ok(()) => match result {
                    Ok(()) => {
                        let active_model = record.clone().into_active_model();
                        let result = CacheService::insert_records(vec![active_model]).await;
                        match result {
                            Ok(_) => {
                                let mut records = self.records.write();
                                records.insert(tag.clone(), record);
                                Ok(())
                            }
                            Err(err) => Err(CacheError::ErrorForward(err.to_string())),
                        }
                    }
                    Err(_) => CacheService::remove_record_by_id(id)
                        .await
                        .map_err(|e| CacheError::ErrorForward(e.to_string())),
                },
                Err(_) => CacheService::remove_record_by_id(id)
                    .await
                    .map_err(|e| CacheError::ErrorForward(e.to_string())),
            };

            let mut pending = self.pending_records.write();
            if Arc::strong_count(&arc_semaphore) <= 2 {
                pending.remove(&tag);
            }
            return result;
        }
        drop(records);

        let channel_id = self.channel.id.clone();
        let uuid = Uuid::new_v4().to_string();
        let path = self.build_file_path(&uuid);
        self.operator
            .write(
                path.clone(),
                bytes,
                WriteMode::Cover,
                Some(EnsureMode::SyncAll),
            )
            .await?;

        let record = CacheRecordsActiveModel {
            id: ActiveValue::NotSet,
            tag: ActiveValue::Set(tag.clone()),
            filename: ActiveValue::Set(uuid.clone()),
            sentence: ActiveValue::Set(sentence),
            channel_id: ActiveValue::Set(channel_id),
        };
        let record = match CacheService::insert_record(record).await {
            Ok(record) => {
                record
            }
            Err(e) => {
                let mut pending = self.pending_records.write();
                if Arc::strong_count(&arc_semaphore) <= 2 {
                    pending.remove(&tag);
                }
                return Err(CacheError::ErrorForward(e.to_string()));
            }
        };

        let mut records = self.records.write();
        records.insert(tag.clone(), record);

        let mut pending = self.pending_records.write();
        if Arc::strong_count(&arc_semaphore) <= 2 {
            pending.remove(&tag);
        }
        Ok(())
    }

    async fn should_update(&self, tag: &String, new_sentence: &String) -> Result<bool, CacheError> {
        let records = self.records.read();
        if !records.contains_key(tag) {
            return Ok(true);
        }
        let record = records.get(tag).unwrap();
        if &record.sentence != new_sentence {
            return Ok(true);
        }
        Ok(false)
    }

    async fn fetch(&self, tag: &String) -> Result<Vec<u8>, CacheError> {
        let records = self.records.read();
        if !records.contains_key(tag) {
            return Err(CacheError::RecordNotExists(tag.clone()));
        }
        let record = records.get(tag).unwrap();
        let path = self.build_file_path(&record.filename);
        drop(records);
        self.operator.read(&path).await
    }

    async fn ensure_single(&self, tag: &String) -> Result<(), CacheError> {
        let records = self.records.read();
        if !records.contains_key(tag) {
            return Err(CacheError::RecordNotExists(tag.clone()));
        }
        let record = records.get(tag).unwrap();
        let path = self.build_file_path(&record.filename);
        drop(records);
        self.operator
            .ensure_single(&path, Duration::from_secs(60))
            .await
    }

    async fn persist(&self) -> Result<(), CacheError> {
        Ok(())
    }

    async fn record(&self, tag: &String) -> Result<CacheRecordsModel, CacheError> {
        let records = self.records.read();
        if !records.contains_key(tag) {
            return Err(CacheError::RecordNotExists(tag.clone()));
        }
        let record = records.get(tag).unwrap().clone();
        Ok(record)
    }

    async fn path(&self, tag: &String) -> Result<String, CacheError> {
        let records = self.records.read();
        if !records.contains_key(tag) {
            return Err(CacheError::RecordNotExists(tag.clone()));
        }
        let record = records.get(tag).unwrap();
        let path = self.build_file_path(&record.filename);
        Ok(path)
    }
}

impl AsyncFileOperator for DefaultAsyncFileOperator {
    async fn write(
        &self,
        path: String,
        bytes: Bytes,
        write_mode: WriteMode,
        ensure_mode: Option<EnsureMode>,
    ) -> Result<(), CacheError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let key = path.clone();
        let (finish_sender, finish_receiver) = oneshot::channel::<()>();
        let request = Arc::new(QueuedRequest {
            path,
            bytes,
            write_mode,
            ensure_mode,
            add_time: now,
            priority: RwLock::new(Priority::Normal),
            finish_sender: Mutex::new(Some(finish_sender)),
            finish_receiver: Mutex::new(Some(finish_receiver)),
        });
        self.stash.write().insert(key, Arc::downgrade(&request));
        self.queue.push(request).await;

        Ok(())
    }

    async fn read(&self, path: &String) -> Result<Vec<u8>, CacheError> {
        let stash = self.stash.read();
        let request = stash.get(path);
        if request.is_none() {
            drop(stash);
            return Self::read_file(path).await;
        }
        let request = request.unwrap().upgrade();
        drop(stash);
        if request.is_none() {
            return Self::read_file(path).await;
        }
        let request = request.unwrap();
        Ok(request.bytes.clone().to_vec())
    }

    async fn ensure_single(&self, path: &String, duration: Duration) -> Result<(), CacheError> {
        let stash = self.stash.read();
        let request = stash.get(path);
        if request.is_none() {
            return Ok(());
        }
        let request = request.unwrap().upgrade();
        drop(stash);
        if request.is_none() {
            return Ok(());
        }
        let request = request.unwrap();
        {
            *request.priority.write() = Priority::Flush;
        }

        let mut finish_receiver = request.finish_receiver.lock();
        if finish_receiver.is_some() {
            let receiver = finish_receiver.take().unwrap();
            drop(finish_receiver);
            timeout(duration, receiver)
                .await?
                .map_err(|e| CacheError::ErrorForward(e.to_string()))?;
        }

        Ok(())
    }
}

impl Priority {
    fn ordinal(&self) -> u8 {
        match self {
            Priority::Normal => 1,
            Priority::Flush => 0,
        }
    }
}

impl Drop for DefaultAsyncFileOperator {
    fn drop(&mut self) {
        self.dispose();
    }
}

impl Ord for QueuedRequest {
    fn cmp(&self, other: &Self) -> Ordering {
        let p1 = self.priority.read();
        let p2 = self.priority.read();
        if *p1 != *p2 {
            return p1.cmp(&p2);
        }
        drop(p1);
        drop(p2);

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let len1 = self.bytes.len() as u64;
        let len2 = other.bytes.len() as u64;
        let delta_time1 = now - self.add_time;
        let delta_time2 = now - other.add_time;
        let p1 = u64_to_unit_float(len1) - 4.0 * u64_to_unit_float(delta_time1);
        let p2 = u64_to_unit_float(len2) - 4.0 * u64_to_unit_float(delta_time2);

        p1.total_cmp(&p2)
    }
}

impl PartialOrd for QueuedRequest {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Priority {
    fn cmp(&self, other: &Self) -> Ordering {
        let o1 = self.ordinal();
        let o2 = other.ordinal();
        if o1 == o2 {
            return Ordering::Equal;
        }

        if self.ordinal() < other.ordinal() {
            return Ordering::Greater;
        }
        Ordering::Less
    }
}

impl PartialOrd for Priority {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Eq for QueuedRequest {}

impl PartialEq for QueuedRequest {
    fn eq(&self, other: &Self) -> bool {
        self.path.eq(&other.path)
    }
}

fn u64_to_unit_float(x: u64) -> f64 {
    let bits = (x >> 11) | (970u64 << 52);
    f64::from_bits(bits)
}

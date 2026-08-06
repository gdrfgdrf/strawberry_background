use crate::domain::models::file_cache_models::{CacheChannel, CacheError, CacheRecord};
use crate::domain::models::storage_models::{ReadFile, WriteFile, WriteMode};
use crate::domain::traits::file_cache_traits::{FileCacheManager, FileCacheManagerFactory};
use crate::domain::traits::storage_traits::StorageManager;
use crate::rkv::rkv_impl::RKV_SERVICE;
use crate::service::config::FileCacheConfig;
use async_trait::async_trait;
use dashmap::DashMap;
use rkv::SingleStore;
use rkv::backend::SafeModeDatabase;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::fs::{File, try_exists};
use tokio::sync::{Mutex, RwLock};
use tracing::{Level, span, Instrument};
use uuid::Uuid;

pub struct SingletonFileCacheManagerFactory<T>
where
    T: Fn(&FileCacheConfig, CacheChannel, Arc<dyn StorageManager>) -> Arc<dyn FileCacheManager>,
{
    pub config: FileCacheConfig,
    map: DashMap<String, Arc<dyn FileCacheManager>>,
    creator: T,
    storage_manager: Arc<dyn StorageManager>,
    single_store: SingleStore<SafeModeDatabase>,
    _session: span::Span,
}

pub struct DefaultFileCacheManager {
    name: String,
    path: String,
    extension: Option<String>,
    save_lock: Mutex<()>,
    auto_save_interval: Duration,
    dirty: Arc<AtomicBool>,
    map: DashMap<String, RwLock<CacheRecord>>,
    storage_manager: Arc<dyn StorageManager>,
    single_store: SingleStore<SafeModeDatabase>,
    _session: span::Span,
}

impl<T> SingletonFileCacheManagerFactory<T>
where
    T: Fn(&FileCacheConfig, CacheChannel, Arc<dyn StorageManager>) -> Arc<dyn FileCacheManager>,
{
    pub fn new(
        config: FileCacheConfig,
        storage_manager: Arc<dyn StorageManager>,
        creator: T,
    ) -> Self {
        let session = span!(Level::INFO, "file-cache-manager-factory");
        let _ = session.enter();
        tracing::debug!(
            base_path = ?config.base_path,
            auto_save_interval_ms = ?config.auto_save_interval.as_millis(),
            channels = ?config.channels,
            "creating file cache manager factory"
        );

        let mut rkv_service = RKV_SERVICE.write().unwrap();
        let rkv_service = rkv_service.as_mut().unwrap();
        let store = rkv_service.init_db("file_cache").unwrap();

        Self {
            config,
            map: DashMap::new(),
            creator,
            storage_manager,
            single_store: store,
            _session: session,
        }
    }
}

impl DefaultFileCacheManager {
    pub fn new(
        path: String,
        auto_save_interval: Duration,
        channel: CacheChannel,
        storage_manager: Arc<dyn StorageManager>,
    ) -> Self {
        let session = span!(Level::INFO, "default-file-cache-manager");
        let _ = session.enter();
        tracing::debug!(
            path = ?path,
            auto_save_interval = ?auto_save_interval.as_millis(),
            channel = ?channel,
            "creating default file cache manager"
        );

        let mut rkv_service = RKV_SERVICE.write().unwrap();
        let rkv_service = rkv_service.as_mut().unwrap();
        let store = rkv_service.init_db("file_cache").unwrap();

        let records = channel.records;
        let map: DashMap<String, RwLock<CacheRecord>> = DashMap::new();
        records.into_iter().for_each(|record| {
            let tag = record.tag.clone();
            map.insert(tag, RwLock::new(record));
        });

        Self {
            name: channel.name,
            path,
            extension: channel.extension,
            save_lock: Mutex::new(()),
            auto_save_interval,
            dirty: Arc::new(AtomicBool::new(false)),
            map,
            storage_manager,
            single_store: store,
            _session: session,
        }
    }

    fn build_path(&self, filename: &String) -> String {
        if self.extension.is_some() {
            return format!(
                "{}/{}.{}",
                self.path,
                filename,
                self.extension.as_ref().unwrap()
            );
        }

        format!("{}/{}", self.path, filename)
    }

    fn make_dirty(&self) {
        self.dirty.store(true, Ordering::SeqCst);
    }

    fn make_clean(&self) {
        self.dirty.store(false, Ordering::SeqCst);
    }

    fn is_dirty(&self) -> bool {
        self.dirty.load(Ordering::SeqCst)
    }

    #[tracing::instrument(skip(self), parent = &self._session)]
    async fn ensure_directory_exists(&self, directory: &String) -> Result<(), CacheError> {
        if !try_exists(directory)
            .await
            .map_err(|e| {
                tracing::debug!(error = %e, "checking directory if exists error");
                CacheError::IO(e.to_string())
            })?
        {
            return tokio::fs::create_dir_all(directory)
                .await
                .map_err(|e| {
                    tracing::debug!(error = %e, "create directory error");
                    CacheError::IO(e.to_string())
                });
        }
        Ok(())
    }

    #[tracing::instrument(skip(self), parent = &self._session)]
    async fn ensure_file_exists(&self, filename: &String) -> Result<(), CacheError> {
        if !try_exists(filename)
            .await
            .map_err(|e| {
                tracing::debug!(error = %e, "checking file if exists error");
                CacheError::IO(e.to_string())
            })?
        {
            let file = File::create_new(filename)
                .await
                .map_err(|e| {
                    tracing::debug!(error = %e, "create file error");
                    CacheError::IO(e.to_string())
                })?;

            file.sync_all()
                .await
                .map_err(|e| {
                    tracing::debug!(error = %e, "sync filesystem error");
                    CacheError::IO(e.to_string())
                })?
        }
        Ok(())
    }

    #[tracing::instrument(skip(self), parent = &self._session)]
    pub fn start_auto_save(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        let store = self.dirty.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(self.auto_save_interval);
            tracing::debug!(interval = ?interval.period().as_millis(), "start ticking");
            
            loop {
                tracing::debug!("ticking");
                interval.tick().await;
                tracing::debug!("ticked");
                if store.load(Ordering::SeqCst) {
                    if let Err(e) = self.persist().await {
                        eprintln!("Failed to auto-save cache channel: {}", e);
                    }
                }
            }
        }.in_current_span())
    }
}

#[async_trait]
impl<T> FileCacheManagerFactory for SingletonFileCacheManagerFactory<T>
where
    T: Fn(&FileCacheConfig, CacheChannel, Arc<dyn StorageManager>) -> Arc<dyn FileCacheManager>
        + Send
        + Sync
        + 'static,
{
    #[tracing::instrument(skip(self), parent = &self._session)]
    async fn create_with_name(
        &self,
        name: String,
        extension: Option<String>,
    ) -> Result<Arc<dyn FileCacheManager>, CacheError> {
        if self.map.contains_key(&name) {
            return Ok(self.map.get(&name).unwrap().clone());
        }
        let channel = self.create_channel(name, extension).await?;
        self.create_with_channel(channel).await
    }

    #[tracing::instrument(skip(self), parent = &self._session)]
    async fn create_channel(
        &self,
        name: String,
        extension: Option<String>,
    ) -> Result<CacheChannel, CacheError> {
        let rkv_service = RKV_SERVICE.read().unwrap();
        let rkv_service = rkv_service.as_ref().unwrap();
        let channel = rkv_service
            .read_rkyv_cache_channel_data(&self.single_store, &name)
            .map_err(|e| {
                tracing::debug!(error = %e, "read cache channel data error");
                CacheError::ErrorForward(e.to_string())
            })?;

        if channel.is_none() {
            let channel = CacheChannel {
                name,
                extension,
                records: Vec::new(),
            };
            return Ok(channel);
        }

        Ok(channel.unwrap())
    }

    #[tracing::instrument(skip(self), parent = &self._session)]
    async fn create_with_channel(
        &self,
        channel: CacheChannel,
    ) -> Result<Arc<dyn FileCacheManager>, CacheError> {
        let name = channel.name.clone();
        if self.map.contains_key(&name) {
            return Ok(self.map.get(&name).unwrap().clone());
        }
        let manager = (self.creator)(&self.config, channel, self.storage_manager.clone());
        self.map.insert(name, manager.clone());

        Ok(manager)
    }

    #[tracing::instrument(skip(self), parent = &self._session)]
    async fn get_with_name(&self, name: &String) -> Result<Arc<dyn FileCacheManager>, CacheError> {
        if !self.map.contains_key(name) {
            return Err(CacheError::ManagerNotExist(name.clone()));
        }
        let manager = self.map.get(name).unwrap();
        Ok(manager.clone())
    }
}

#[async_trait]
impl FileCacheManager for DefaultFileCacheManager {
    #[tracing::instrument(skip(self, bytes), parent = &self._session)]
    async fn cache(
        &self,
        tag: String,
        sentence: String,
        bytes: &Vec<u8>,
    ) -> Result<(), CacheError> {
        if self.map.contains_key(&tag) {
            tracing::debug!("tag is existing in map, overwriting");
            
            let entry = self.map.get_mut(&tag).ok_or(CacheError::TagNotExist(tag))?;
            let mut record = entry
                .try_write()
                .map_err(|e| CacheError::Lock(e.to_string()))?;

            let path = self.build_path(&record.filename);
            self.ensure_directory_exists(&self.path).await?;
            self.ensure_file_exists(&path).await?;

            let write_file = WriteFile {
                path,
                mode: WriteMode::Cover,
                timeout: Duration::from_secs(60),
                ensure_mode: None,
                data: bytes,
            };

            return self
                .storage_manager
                .write(write_file)
                .await
                .inspect(|_| {
                    record.sentence = sentence;
                    record.size = bytes.len();
                    self.make_dirty();
                })
                .map_err(|e| {
                    tracing::debug!(error = %e, "write file error");
                    CacheError::from(e)
                });
        }

        let filename = Uuid::new_v4().to_string();
        let path = self.build_path(&filename);
        self.ensure_directory_exists(&self.path).await?;
        self.ensure_file_exists(&path).await?;

        let write_file = WriteFile {
            path,
            mode: WriteMode::Cover,
            timeout: Duration::from_secs(60),
            ensure_mode: None,
            data: bytes,
        };

        self.storage_manager
            .write(write_file)
            .await
            .inspect(|_| {
                let record = CacheRecord {
                    tag: tag.clone(),
                    filename,
                    size: bytes.len(),
                    sentence,
                };

                self.map.insert(tag, RwLock::new(record));
                self.make_dirty();
            })
            .map_err(|e| {
                tracing::debug!(error = %e, "write file error");
                CacheError::from(e)
            })
    }

    #[tracing::instrument(skip(self), parent = &self._session)]
    async fn should_update(&self, tag: &String, sentence: &String) -> Result<bool, CacheError> {
        let entry = self
            .map
            .get_mut(tag)
            .ok_or(CacheError::TagNotExist(tag.clone()))?;
        let record = entry
            .try_write()
            .map_err(|e| CacheError::Lock(e.to_string()))?;
        let filename = &record.filename;
        if !try_exists(self.build_path(filename))
            .await
            .map_err(|e| {
                tracing::debug!(error = %e, "check if file exists error");
                CacheError::IO(e.to_string())
            })?
        {
            return Ok(true);
        }

        Ok(record.sentence != *sentence)
    }

    #[tracing::instrument(skip(self), parent = &self._session)]
    async fn fetch(&self, tag: &String) -> Result<Vec<u8>, CacheError> {
        let entry = self
            .map
            .get_mut(tag)
            .ok_or(CacheError::TagNotExist(tag.clone()))?;
        let record = entry
            .try_write()
            .map_err(|e| CacheError::Lock(e.to_string()))?;
        let filename = &record.filename;
        let path = self.build_path(filename);

        if !try_exists(&path)
            .await
            .map_err(|e| {
                tracing::debug!(error = %e, "check if file exists error");
                CacheError::IO(e.to_string())
            })?
        {
            return Err(CacheError::FileNotExist(path));
        }

        let read_file = ReadFile::path(path);
        self.storage_manager
            .read(read_file)
            .await
            .map_err(|e| {
                tracing::debug!(error = %e, "read file error");
                CacheError::from(e)
            })
    }

    async fn flush(&self, tag: &String) -> Result<(), CacheError> {
        Ok(())
    }

    #[tracing::instrument(skip(self), parent = &self._session)]
    async fn persist(&self) -> Result<(), CacheError> {
        let is_dirty = self.is_dirty();
        tracing::debug!(is_dirty = ?is_dirty, "persisting");
        if !is_dirty {
            return Ok(());
        }

        let _ = self.save_lock.lock();

        let mut records: Vec<CacheRecord> = Vec::new();
        for record in &self.map {
            let record = record.read().await;
            let record = record.clone();
            records.push(record);
        }

        let channel = CacheChannel {
            name: self.name.clone(),
            extension: self.extension.clone(),
            records,
        };

        let rkv_service = RKV_SERVICE.read().unwrap();
        let rkv_service = rkv_service.as_ref().unwrap();
        rkv_service
            .write_rkyv_cache_channel_data(&self.single_store, &self.name, &channel)
            .map_err(|e| {
                tracing::debug!(error = %e, "writing channel datas error");
                CacheError::ErrorForward(e.to_string())
            })?;
        self.make_clean();
        Ok(())
    }

    #[tracing::instrument(skip(self), parent = &self._session)]
    async fn record(&self, tag: &String) -> Result<CacheRecord, CacheError> {
        let entry = self
            .map
            .get_mut(tag)
            .ok_or(CacheError::TagNotExist(tag.clone()))?;
        let record = entry
            .try_write()
            .map_err(|e| CacheError::Lock(e.to_string()))?;
        let record = record.clone();
        Ok(record)
    }

    #[tracing::instrument(skip(self), parent = &self._session)]
    async fn path(&self, tag: &String) -> Result<String, CacheError> {
        let entry = self
            .map
            .get_mut(tag)
            .ok_or(CacheError::TagNotExist(tag.clone()))?;
        let record = entry
            .try_write()
            .map_err(|e| CacheError::Lock(e.to_string()))?;
        let filename = &record.filename;
        let path = self.build_path(filename);

        if !try_exists(&path)
            .await
            .map_err(|e| {
                tracing::debug!(error = %e, "check if file exists error");
                CacheError::IO(e.to_string())
            })?
        {
            return Err(CacheError::FileNotExist(path));
        }

        Ok(path)
    }
}

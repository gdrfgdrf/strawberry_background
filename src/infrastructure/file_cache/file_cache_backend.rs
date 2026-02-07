use crate::domain::models::file_cache_models::{CacheChannel, CacheError, CacheRecord};
use crate::domain::traits::file_cache_traits::{FileCacheManager, FileCacheManagerFactory};
use crate::service::config::FileCacheConfig;
use async_trait::async_trait;
use dashmap::DashMap;
use rkyv::rancor::Error;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::fs::{File, read, try_exists};
use tokio::sync::{Mutex, RwLock};
use tokio::time::timeout;
use uuid::Uuid;

pub struct SingletonFileCacheManagerFactory<T>
where
    T: Fn(&FileCacheConfig, CacheChannel) -> Arc<dyn FileCacheManager>,
{
    pub config: FileCacheConfig,
    map: DashMap<String, Arc<dyn FileCacheManager>>,
    creator: T,
}

pub struct DefaultFileCacheManager {
    name: String,
    path: String,
    extension: Option<String>,
    save_lock: Mutex<()>,
    auto_save_interval: Duration,
    dirty: Arc<AtomicBool>,
    map: DashMap<String, RwLock<CacheRecord>>,
}

impl<T> SingletonFileCacheManagerFactory<T>
where
    T: Fn(&FileCacheConfig, CacheChannel) -> Arc<dyn FileCacheManager>,
{
    pub fn new(config: FileCacheConfig, creator: T) -> Self {
        Self {
            config,
            map: DashMap::new(),
            creator,
        }
    }

    fn get_channel_path(&self, name: &String) -> String {
        format!("{}/{}/channel.rkyv", self.config.base_path, name)
    }
}

impl DefaultFileCacheManager {
    pub fn new(path: String, auto_save_interval: Duration, channel: CacheChannel) -> Self {
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
        }
    }

    fn get_channel_path(&self) -> String {
        format!("{}/channel.rkyv", self.path)
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

    async fn ensure_directory_exist(&self, directory: &String) -> Result<(), CacheError> {
        if !try_exists(directory)
            .await
            .map_err(|e| CacheError::IO(e.to_string()))?
        {
            return tokio::fs::create_dir_all(directory)
                .await
                .map_err(|e| CacheError::IO(e.to_string()));
        }
        Ok(())
    }

    async fn ensure_file_exist(&self, filename: &String) -> Result<(), CacheError> {
        if !try_exists(filename)
            .await
            .map_err(|e| CacheError::IO(e.to_string()))?
        {
            let file = File::create_new(filename)
                .await
                .map_err(|e| CacheError::IO(e.to_string()))?;

            file.sync_all()
                .await
                .map_err(|e| CacheError::IO(e.to_string()))?
        }
        Ok(())
    }

    pub fn start_auto_save(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        let store = self.dirty.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(self.auto_save_interval);
            loop {
                interval.tick().await;
                if store.load(Ordering::SeqCst) {
                    if let Err(e) = self.persist().await {
                        eprintln!("Failed to auto-save cache channel: {}", e);
                    }
                }
            }
        })
    }
}

#[async_trait]
impl<T> FileCacheManagerFactory for SingletonFileCacheManagerFactory<T>
where
    T: Fn(&FileCacheConfig, CacheChannel) -> Arc<dyn FileCacheManager> + Send + Sync + 'static,
{
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

    async fn create_channel(
        &self,
        name: String,
        extension: Option<String>,
    ) -> Result<CacheChannel, CacheError> {
        let channel_path = self.get_channel_path(&name);
        let exists = try_exists(&channel_path)
            .await
            .map_err(|e| CacheError::IO(e.to_string()))?;
        if !exists {
            let channel = CacheChannel {
                name,
                extension,
                records: Vec::new(),
            };
            return Ok(channel);
        }

        let data = read(&channel_path)
            .await
            .map_err(|e| CacheError::IO(e.to_string()))?;
        let channel = rkyv::from_bytes::<CacheChannel, Error>(&data)
            .map_err(|e| CacheError::IO(e.to_string()))?;

        Ok(channel)
    }

    async fn create_with_channel(
        &self,
        channel: CacheChannel,
    ) -> Result<Arc<dyn FileCacheManager>, CacheError> {
        let name = channel.name.clone();
        if self.map.contains_key(&name) {
            return Ok(self.map.get(&name).unwrap().clone());
        }
        let manager = (self.creator)(&self.config, channel);
        self.map.insert(name, manager.clone());

        Ok(manager)
    }

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
    async fn cache(
        &self,
        tag: String,
        sentence: String,
        bytes: &Vec<u8>,
    ) -> Result<(), CacheError> {
        if self.map.contains_key(&tag) {
            let entry = self.map.get_mut(&tag).ok_or(CacheError::TagNotExist(tag))?;
            let mut record = entry
                .try_write()
                .map_err(|e| CacheError::Lock(e.to_string()))?;

            let path = self.build_path(&record.filename);
            self.ensure_directory_exist(&self.path).await?;
            self.ensure_file_exist(&path).await?;

            return match timeout(Duration::from_secs(60), tokio::fs::write(&path, &bytes)).await {
                Ok(Ok(())) => {
                    record.sentence = sentence;
                    record.size = bytes.len();
                    self.make_dirty();

                    Ok(())
                }
                Ok(Err(e)) => Err(CacheError::IO(e.to_string())),
                Err(e) => Err(CacheError::Timeout(e.to_string())),
            };
        }

        let filename = Uuid::new_v4().to_string();
        let path = self.build_path(&filename);
        self.ensure_directory_exist(&self.path).await?;
        self.ensure_file_exist(&path).await?;

        match timeout(Duration::from_secs(60), tokio::fs::write(&path, &bytes)).await {
            Ok(Ok(())) => {
                let record = CacheRecord {
                    tag: tag.clone(),
                    filename,
                    size: bytes.len(),
                    sentence,
                };

                self.map.insert(tag, RwLock::new(record));
                self.make_dirty();

                Ok(())
            }
            Ok(Err(e)) => Err(CacheError::IO(e.to_string())),
            Err(e) => Err(CacheError::Timeout(e.to_string())),
        }
    }

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
            .map_err(|e| CacheError::IO(e.to_string()))?
        {
            return Ok(true);
        }

        Ok(record.sentence != *sentence)
    }

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
            .map_err(|e| CacheError::IO(e.to_string()))?
        {
            return Err(CacheError::FileNotExist(path));
        }

        match timeout(Duration::from_secs(60), read(&path)).await {
            Ok(Ok(data)) => Ok(data),
            Ok(Err(e)) => Err(CacheError::IO(e.to_string())),
            Err(e) => Err(CacheError::Timeout(e.to_string())),
        }
    }

    async fn flush(&self, tag: &String) -> Result<(), CacheError> {
        if !self.map.contains_key(tag) {
            return Err(CacheError::TagNotExist(tag.clone()));
        }

        let record = self.map.remove(tag).unwrap();
        self.make_dirty();

        let record = record.1.into_inner();
        let path = self.build_path(&record.filename);

        if try_exists(&path)
            .await
            .map_err(|e| CacheError::IO(e.to_string()))?
        {
            return tokio::fs::remove_file(path)
                .await
                .map_err(|e| CacheError::IO(e.to_string()));
        }

        Ok(())
    }

    async fn persist(&self) -> Result<(), CacheError> {
        if !self.is_dirty() {
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

        let bytes = rkyv::to_bytes::<Error>(&channel)
            .map_err(|e| CacheError::Serialization(e.to_string()))?;

        let channel_path = self.get_channel_path();
        self.ensure_directory_exist(&self.path).await?;
        self.ensure_file_exist(&channel_path).await?;

        match timeout(
            Duration::from_secs(60),
            tokio::fs::write(&channel_path, &bytes),
        )
        .await
        {
            Ok(Ok(())) => {
                self.make_clean();
                Ok(())
            }
            Ok(Err(e)) => Err(CacheError::IO(e.to_string())),
            Err(e) => Err(CacheError::Timeout(e.to_string())),
        }
    }

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
            .map_err(|e| CacheError::IO(e.to_string()))?
        {
            return Err(CacheError::FileNotExist(path));
        }

        return Ok(path);
    }
}

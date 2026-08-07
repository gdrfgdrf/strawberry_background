use crate::domain::models::file_cache_models::CacheError;
use crate::domain::models::storage_models::{EnsureMode, WriteMode};
use bytes::Bytes;
use std::time::Duration;
use crate::db::models::preclude::CacheRecordsModel;

pub trait AsyncFileOperator {
    async fn write(
        &self,
        path: String,
        bytes: Bytes,
        write_mode: WriteMode,
        ensure_mode: Option<EnsureMode>,
    ) -> Result<(), CacheError>;
    async fn read(&self, path: &String) -> Result<Bytes, CacheError>;
    async fn flush_single(&self, path: &String, timeout: Duration) -> Result<(), CacheError>;
}

pub trait AsyncFileCacheManager {
    async fn cache(&self, tag: String, sentence: String, bytes: Bytes) -> Result<(), CacheError>;
    async fn should_update(&self, tag: &String, new_sentence: &String) -> Result<bool, CacheError>;
    async fn fetch(&self, tag: &String) -> Result<Bytes, CacheError>;
    async fn persist(&self) -> Result<(), CacheError>;

    async fn record(&self, tag: &String) -> Result<CacheRecordsModel, CacheError>;
    async fn path(&self, tag: &String) -> Result<String, CacheError>;
}

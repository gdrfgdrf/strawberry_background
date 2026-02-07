use crate::domain::models::file_cache_models::{CacheChannel, CacheError, CacheRecord};
use async_trait::async_trait;
use std::sync::Arc;

#[async_trait]
pub trait FileCacheManagerFactory: Send + Sync + 'static {
    async fn create_with_name(
        &self,
        name: String,
        extension: Option<String>,
    ) -> Result<Arc<dyn FileCacheManager>, CacheError>;
    
    async fn create_channel(
        &self,
        name: String,
        extension: Option<String>,
    ) -> Result<CacheChannel, CacheError>;

    async fn create_with_channel(
        &self,
        channel: CacheChannel,
    ) -> Result<Arc<dyn FileCacheManager>, CacheError>;
    
    async fn get_with_name(&self, name: &String) -> Result<Arc<dyn FileCacheManager>, CacheError>;
}

#[async_trait]
pub trait FileCacheManager: Send + Sync + 'static {
    async fn cache(&self, tag: String, sentence: String, bytes: &Vec<u8>) -> Result<(), CacheError>;
    async fn should_update(&self, tag: &String, sentence: &String) -> Result<bool, CacheError>;
    async fn fetch(&self, tag: &String) -> Result<Vec<u8>, CacheError>;
    async fn flush(&self, tag: &String) -> Result<(), CacheError>;
    async fn persist(&self) -> Result<(), CacheError>;

    async fn record(&self, tag: &String) -> Result<CacheRecord, CacheError>;
    async fn path(&self, tag: &String) -> Result<String, CacheError>;
}

use async_trait::async_trait;
use crate::domain::models::storage_models::{ReadFile, StorageError, WriteFile};

#[async_trait]
pub trait StorageManager: Send + Sync + 'static {
    async fn read(&self, request: ReadFile) -> Result<Vec<u8>, StorageError>;
    async fn write(&self, request: WriteFile) -> Result<(), StorageError>;
}
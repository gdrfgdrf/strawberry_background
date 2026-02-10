use crate::domain::models::file_cache_models::CacheError;
use crate::domain::models::storage_models::StorageError;

impl From<StorageError> for CacheError {
    fn from(value: StorageError) -> Self {
        CacheError::ErrorForward(value.to_string())
    }
}
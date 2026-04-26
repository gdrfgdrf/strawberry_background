use std::sync::PoisonError;
use crate::domain::models::coordinator_models::{CategorizerError, CoordinatorError, DiscoverError, QueuerError, RegistryError};
use crate::domain::models::file_cache_models::CacheError;
use crate::domain::models::storage_models::StorageError;
use crate::utils::waiter::TimeoutError;

impl From<StorageError> for CacheError {
    fn from(value: StorageError) -> Self {
        CacheError::ErrorForward(value.to_string())
    }
}

impl<T> From<PoisonError<T>> for CoordinatorError {
    fn from(value: PoisonError<T>) -> Self {
        CoordinatorError::ErrorForward(value.to_string())
    }
}

impl<T> From<PoisonError<T>> for DiscoverError {
    fn from(value: PoisonError<T>) -> Self {
        DiscoverError::ErrorForward(value.to_string())
    }
}

impl<T> From<PoisonError<T>> for QueuerError {
    fn from(value: PoisonError<T>) -> Self {
        QueuerError::ErrorForward(value.to_string())
    }
}

impl From<RegistryError> for CoordinatorError {
    fn from(value: RegistryError) -> Self {
        CoordinatorError::ErrorForward(value.to_string())
    }
}

impl From<RegistryError> for DiscoverError {
    fn from(value: RegistryError) -> Self {
        DiscoverError::ErrorForward(value.to_string())
    }
}

impl From<RegistryError> for QueuerError {
    fn from(value: RegistryError) -> Self {
        QueuerError::ErrorForward(value.to_string())
    }
}

impl From<DiscoverError> for CoordinatorError {
    fn from(value: DiscoverError) -> Self {
        CoordinatorError::ErrorForward(value.to_string())
    }
}

impl From<QueuerError> for CoordinatorError {
    fn from(value: QueuerError) -> Self {
        CoordinatorError::ErrorForward(value.to_string())
    }
}

impl From<TimeoutError> for CoordinatorError {
    fn from(value: TimeoutError) -> Self {
        CoordinatorError::ErrorForward(value.to_string())
    }
}

impl From<TimeoutError> for DiscoverError {
    fn from(value: TimeoutError) -> Self {
        DiscoverError::ErrorForward(value.to_string())
    }
}

impl From<CategorizerError> for QueuerError {
    fn from(value: CategorizerError) -> Self {
        QueuerError::ErrorForward(value.to_string())
    }
}

impl From<DiscoverError> for QueuerError {
    fn from(value: DiscoverError) -> Self {
        QueuerError::ErrorForward(value.to_string())
    }
}
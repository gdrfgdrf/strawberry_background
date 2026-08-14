use std::io::Error;
use std::sync::PoisonError;
#[cfg(target_os = "android")]
use android_media::MicError;
use rodio::decoder::DecoderError;
use rodio::DeviceSinkError;
use rodio::source::SeekError;
use tokio::time::error::Elapsed;
use crate::db::initializer::DatabaseError;
use crate::domain::models::audio_models::AudioError;
use crate::domain::models::cookie_models::CookieError;
use crate::domain::models::coordinator_models::{CategorizerError, CoordinatorError, DiscoverError, QueuerError, RegistryError};
use crate::domain::models::file_cache_models::CacheError;
use crate::domain::models::http_models::HttpClientError;
use crate::domain::models::storage_models::StorageError;
use crate::utils::waiter::TimeoutError;

impl From<StorageError> for CacheError {
    fn from(value: StorageError) -> Self {
        CacheError::ErrorForward(value.to_string())
    }
}

impl From<Error> for CacheError {
    fn from(value: Error) -> Self {
        CacheError::ErrorForward(value.to_string())
    }
}

impl From<Elapsed> for CacheError {
    fn from(value: Elapsed) -> Self {
        CacheError::ErrorForward(value.to_string())
    }
}

impl From<DatabaseError> for CacheError {
    fn from(value: DatabaseError) -> Self {
        CacheError::ErrorForward(value.to_string())
    }
}

impl From<CacheError> for String {
    fn from(value: CacheError) -> Self {
        value.to_string()
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

impl From<DeviceSinkError> for AudioError {
    fn from(value: DeviceSinkError) -> Self {
        AudioError::ErrorForward(value.to_string())
    }
}

impl From<DecoderError> for AudioError {
    fn from(value: DecoderError) -> Self {
        AudioError::ErrorForward(value.to_string())
    }
}

impl From<SeekError> for AudioError {
    fn from(value: SeekError) -> Self {
        AudioError::ErrorForward(value.to_string())
    }
}

impl From<DatabaseError> for CookieError {
    fn from(value: DatabaseError) -> Self {
        CookieError::ErrorForward(value.to_string())
    }
}

impl From<CookieError> for HttpClientError {
    fn from(value: CookieError) -> Self {
        HttpClientError::ErrorForward(value.to_string())
    }
}

#[cfg(target_os = "android")]
impl From<MicError> for AudioError {
    fn from(value: MicError) -> Self {
        AudioError::ErrorForward(value.to_string())
    }
}
use crate::domain::models::storage_models::{
    EnsureMode, ReadFile, StorageError, WriteFile, WriteMode,
};
use crate::domain::traits::storage_traits::StorageManager;
use crate::utils::keyed_rw_lock::KeyedRwLock;
use async_trait::async_trait;
use tokio::fs::{OpenOptions, read};
use tokio::io::AsyncWriteExt;
use tokio::time::timeout;

macro_rules! match_timeout {
    ( $x:expr, $y:expr ) => {{
        match timeout($x, $y).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(StorageError::IOError(e.to_string())),
            Err(timeout) => Err(StorageError::Timeout(timeout.to_string())),
        }
    }};
}

pub struct AsyncStorageManager {
    keys: KeyedRwLock<()>,
}

impl AsyncStorageManager {
    pub fn new() -> Self {
        Self {
            keys: KeyedRwLock::new(),
        }
    }
}

#[async_trait]
impl StorageManager for AsyncStorageManager {
    async fn read(&self, request: ReadFile) -> Result<Vec<u8>, StorageError> {
        self.keys
            .read(&request.path.clone(), |_| async {
                match timeout(request.timeout, read(request.path)).await {
                    Ok(Ok(data)) => Ok(data),
                    Ok(Err(e)) => Err(StorageError::IOError(e.to_string())),
                    Err(timeout) => Err(StorageError::Timeout(timeout.to_string())),
                }
            })
            .await
            .await
    }

    async fn write(&self, request: WriteFile) -> Result<(), StorageError> {
        self.keys
            .write(&request.path.clone(), |_| async {
                let mut file = OpenOptions::new()
                    .create(true)
                    .append(request.mode == WriteMode::Append)
                    .write(request.mode == WriteMode::Cover)
                    .open(request.path)
                    .await
                    .map_err(|e| StorageError::IOError(e.to_string()))?;

                return match timeout(request.timeout, file.write_all(&request.data)).await {
                    Ok(Ok(())) => {
                        if request.ensure_mode.is_some() {
                            return match request.ensure_mode.unwrap() {
                                EnsureMode::Flush => {
                                    match_timeout!(request.timeout, file.flush())
                                }
                                EnsureMode::SyncData => {
                                    match_timeout!(request.timeout, file.sync_data())
                                }
                                EnsureMode::SyncAll => {
                                    match_timeout!(request.timeout, file.sync_all())
                                }
                            };
                        }
                        Ok(())
                    }
                    Ok(Err(e)) => Err(StorageError::IOError(e.to_string())),
                    Err(timeout) => Err(StorageError::Timeout(timeout.to_string())),
                };
            })
            .await
            .await
    }
}

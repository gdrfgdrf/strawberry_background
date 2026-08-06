use crate::domain::models::monitor_models::{
    EventStage, MonitorEvent, MonitorHttpData, MonitorStorageData, Progress,
};
use crate::domain::models::storage_models::{
    EnsureMode, ReadFile, StorageError, WriteFile, WriteMode,
};
use crate::domain::traits::monitor_traits::Monitor;
use crate::domain::traits::storage_traits::StorageManager;
use crate::monitor::monitor_service::monitoring;
use crate::utils::keyed_rw_lock::KeyedRwLock;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::fs::{OpenOptions, read, try_exists};
use tokio::io::AsyncWriteExt;
use tokio::time::timeout;
use tracing::{Level, span};

macro_rules! match_timeout {
    ( $x:expr, $y:expr ) => {{
        match timeout($x, $y).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(StorageError::IOError(e.to_string())),
            Err(timeout) => Err(StorageError::Timeout(timeout.to_string())),
        }
    }};
}

fn send_monitor_event(
    monitor: Arc<dyn Monitor>,
    path: &String,
    stage: EventStage,
    progress_values: Option<(u64, u64, u64)>,
) {
    let mut progress_option: Option<Progress> = None;
    if progress_values.is_some() {
        let values = progress_values.unwrap();
        progress_option = Some(Progress {
            value: values.0,
            total: values.1,
            delta: values.2,
        })
    }
    let monitor_storage_data = progress_option.map(|progress| MonitorStorageData { progress });
    let event = MonitorEvent::Storage {
        stage,
        path: path.to_string(),
        data: monitor_storage_data,
    };
    monitor.send(event);
}

pub struct AsyncStorageManager {
    keys: KeyedRwLock<()>,
    _session: span::Span,
}

impl AsyncStorageManager {
    pub fn new() -> Self {
        Self {
            keys: KeyedRwLock::new(),
            _session: span!(Level::INFO, "async-storage-manager"),
        }
    }
}

#[async_trait]
impl StorageManager for AsyncStorageManager {
    #[tracing::instrument(skip(self))]
    async fn read(&self, request: ReadFile) -> Result<Vec<u8>, StorageError> {
        let path = request.path;
        let exists = try_exists(&path).await.map_err(|e| {
            tracing::error!(error = %e, "check if file exists error");
            StorageError::IOError(e.to_string())
        })?;

        monitoring(|monitor| {
            send_monitor_event(monitor, &path, EventStage::Started, None);
        });

        if !exists {
            monitoring(|monitor| {
                send_monitor_event(monitor, &path, EventStage::Failed, None);
            });
            return Err(StorageError::NotExist(path.clone()));
        }

        self.keys
            .read(&path, |_| async {
                match timeout(request.timeout, read(path.clone())).await {
                    Ok(Ok(data)) => Ok(data),
                    Ok(Err(e)) => {
                        tracing::error!(error = %e, "read file error");
                        Err(StorageError::IOError(e.to_string()))
                    }
                    Err(timeout) => {
                        tracing::error!(error = %timeout, "read file timeout");
                        Err(StorageError::Timeout(timeout.to_string()))
                    }
                }
            })
            .await
            .await
            .inspect(|_| {
                monitoring(|monitor| {
                    send_monitor_event(monitor, &path, EventStage::Finished, None);
                })
            })
            .inspect_err(|e| {
                tracing::error!(error = %e, "read file error");
                monitoring(|monitor| {
                    send_monitor_event(monitor, &path, EventStage::Failed, None);
                })
            })
    }

    #[tracing::instrument(skip(self, request))]
    async fn write<'a>(&self, request: WriteFile<'a>) -> Result<(), StorageError> {
        tracing::debug!(
            path = ?request.path, 
            mode = ?request.mode, 
            timeout = ?request.timeout.as_millis(),
            ensure_mode = ?request.ensure_mode, 
            data_length = ?request.data.len(), 
            "writing file"
        );

        let path = request.path;

        monitoring(|monitor| {
            send_monitor_event(monitor, &path, EventStage::Started, None);
        });

        self.keys
            .write(&path.clone(), |_| async {
                let mut file = OpenOptions::new()
                    .create(true)
                    .append(request.mode == WriteMode::Append)
                    .write(request.mode == WriteMode::Cover)
                    .open(path.clone())
                    .await
                    .map_err(|e| {
                        tracing::error!(error = %e, "open file error");
                        StorageError::IOError(e.to_string())
                    })?;

                return match timeout(request.timeout, file.write_all(request.data)).await {
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
                    Ok(Err(e)) => {
                        tracing::error!(error = %e, "write file error");
                        Err(StorageError::IOError(e.to_string()))
                    },
                    Err(timeout) => {
                        tracing::error!(error = %timeout, "write file timeout");
                        Err(StorageError::Timeout(timeout.to_string()))
                    },
                };
            })
            .await
            .await
            .inspect(|_| {
                monitoring(|monitor| {
                    send_monitor_event(monitor, &path, EventStage::Finished, None);
                })
            })
            .inspect_err(|e| {
                tracing::error!(error = %e, "write file error");
                monitoring(|monitor| {
                    send_monitor_event(monitor, &path, EventStage::Failed, None);
                })
            })
    }
}

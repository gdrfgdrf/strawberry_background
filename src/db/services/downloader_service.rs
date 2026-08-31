use crate::db::initializer::{DB, DatabaseError};
use crate::db::models::downloader_records::{DownloaderPriority, DownloaderStatus};
use crate::db::models::preclude::*;
use chrono::Utc;
use sea_orm::sea_query::OnConflict;
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter, TransactionTrait,
};

pub struct NewDownloaderRecord {
    pub request_id: String,
    pub channel_id: i64,
    pub url: String,
    pub path: String,
    pub priority: DownloaderPriority,
    pub downloaded: i64,
    pub status: DownloaderStatus,
}

pub struct DownloaderService {}

impl DownloaderService {
    pub async fn upsert(record: NewDownloaderRecord) -> Result<(), DatabaseError> {
        let db = DB.read().await;
        if db.is_none() {
            return Err(DatabaseError::NotInitialized);
        }
        let db = db.as_ref().unwrap();
        let now = Utc::now().fixed_offset();

        let active_model = DownloaderRecordsActiveModel {
            id: ActiveValue::NotSet,
            request_id: ActiveValue::Set(record.request_id),
            channel_id: ActiveValue::Set(record.channel_id),
            url: ActiveValue::Set(record.url),
            path: ActiveValue::Set(record.path),
            priority: ActiveValue::Set(record.priority),
            downloaded: ActiveValue::Set(record.downloaded),
            expected_length: ActiveValue::Set(None),
            status: ActiveValue::Set(record.status),
            retried_count: ActiveValue::Set(0),
            error_message: ActiveValue::Set(None),
            created_at: ActiveValue::Set(now),
            updated_at: ActiveValue::Set(now),
        };

        let transaction = db.begin().await?;
        DownloaderRecords::insert(active_model)
            .on_conflict(
                OnConflict::column(DownloaderRecordsColumn::RequestId)
                    .update_columns([
                        DownloaderRecordsColumn::ChannelId,
                        DownloaderRecordsColumn::Url,
                        DownloaderRecordsColumn::Path,
                        DownloaderRecordsColumn::Priority,
                        DownloaderRecordsColumn::Downloaded,
                        DownloaderRecordsColumn::ExpectedLength,
                        DownloaderRecordsColumn::Status,
                        DownloaderRecordsColumn::RetriedCount,
                        DownloaderRecordsColumn::ErrorMessage,
                        DownloaderRecordsColumn::UpdatedAt,
                    ])
                    .to_owned(),
            )
            .exec_without_returning(&transaction)
            .await?;
        transaction.commit().await?;

        Ok(())
    }

    pub async fn update_progress(
        request_id: &str,
        downloaded: i64,
        expected_length: Option<i64>,
    ) -> Result<(), DatabaseError> {
        let db = DB.read().await;
        if db.is_none() {
            return Err(DatabaseError::NotInitialized);
        }
        let db = db.as_ref().unwrap();

        let existing = DownloaderRecords::find()
            .filter(DownloaderRecordsColumn::RequestId.eq(request_id))
            .one(db)
            .await?;
        let Some(existing) = existing else {
            return Ok(());
        };

        let mut active_model: DownloaderRecordsActiveModel = existing.into();
        active_model.downloaded = ActiveValue::Set(downloaded);
        active_model.expected_length = ActiveValue::Set(expected_length);
        active_model.updated_at = ActiveValue::Set(Utc::now().fixed_offset());
        active_model.update(db).await?;

        Ok(())
    }

    pub async fn update_status(
        request_id: &str,
        status: DownloaderStatus,
        downloaded: i64,
        retried_count: i32,
        error_message: Option<String>,
    ) -> Result<(), DatabaseError> {
        let db = DB.read().await;
        if db.is_none() {
            return Err(DatabaseError::NotInitialized);
        }
        let db = db.as_ref().unwrap();

        let existing = DownloaderRecords::find()
            .filter(DownloaderRecordsColumn::RequestId.eq(request_id))
            .one(db)
            .await?;
        let Some(existing) = existing else {
            return Ok(());
        };

        let mut active_model: DownloaderRecordsActiveModel = existing.into();
        active_model.status = ActiveValue::Set(status);
        active_model.downloaded = ActiveValue::Set(downloaded);
        active_model.retried_count = ActiveValue::Set(retried_count);
        active_model.error_message = ActiveValue::Set(error_message);
        active_model.updated_at = ActiveValue::Set(Utc::now().fixed_offset());
        active_model.update(db).await?;

        Ok(())
    }

    pub async fn remove_by_request_id(request_id: &str) -> Result<(), DatabaseError> {
        let db = DB.read().await;
        if db.is_none() {
            return Err(DatabaseError::NotInitialized);
        }
        let db = db.as_ref().unwrap();
        let transaction = db.begin().await?;
        DownloaderRecords::delete_many()
            .filter(DownloaderRecordsColumn::RequestId.eq(request_id))
            .exec(&transaction)
            .await?;
        transaction.commit().await?;

        Ok(())
    }

    pub async fn find_resumable() -> Result<Vec<DownloaderRecordsModel>, DatabaseError> {
        let db = DB.read().await;
        if db.is_none() {
            return Err(DatabaseError::NotInitialized);
        }
        let db = db.as_ref().unwrap();
        let records = DownloaderRecords::find()
            .filter(DownloaderRecordsColumn::Status.is_in([
                DownloaderStatus::Enqueued,
                DownloaderStatus::Running,
                DownloaderStatus::Paused,
                DownloaderStatus::WaitForRetry,
            ]))
            .all(db)
            .await?;

        Ok(records)
    }
}

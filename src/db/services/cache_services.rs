use crate::all_columns;
use crate::db::initializer::{DB, DatabaseError};
use crate::db::models::preclude::*;
use sea_orm::sea_query::IntoIden;
use sea_orm::sea_query::OnConflict;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, ModelTrait, QueryFilter, TransactionTrait,
};
use sea_orm::Iterable;
use std::collections::HashMap;

pub struct CacheService {}

impl CacheService {
    pub async fn find_channel_by_id(id: i64) -> Result<Option<CacheChannelsModel>, DatabaseError> {
        let db = DB.read();
        if db.is_none() {
            return Err(DatabaseError::NotInitialized);
        }
        let db = db.as_ref().unwrap();
        let channel = CacheChannels::find()
            .filter(CacheChannelsColumn::Id.eq(&id))
            .one(db)
            .await?;
        Ok(channel)
    }

    pub async fn find_channel_by_name(
        name: String,
    ) -> Result<Option<CacheChannelsModel>, DatabaseError> {
        let db = DB.read();
        if db.is_none() {
            return Err(DatabaseError::NotInitialized);
        }
        let db = db.as_ref().unwrap();
        let channel = CacheChannels::find()
            .filter(CacheChannelsColumn::Name.eq(&name))
            .one(db)
            .await?;
        Ok(channel)
    }

    pub async fn find_channels_by_names(
        names: Vec<String>,
    ) -> Result<Option<Vec<Option<CacheChannelsModel>>>, DatabaseError> {
        let db = DB.read();
        if db.is_none() {
            return Err(DatabaseError::NotInitialized);
        }
        let db = db.as_ref().unwrap();

        let channels = CacheChannels::find()
            .filter(CacheChannelsColumn::Name.is_in(&names))
            .all(db)
            .await?;
        let map = channels
            .into_iter()
            .map(|channel| (channel.name.clone(), channel))
            .collect::<HashMap<String, CacheChannelsModel>>();
        let ordered = names
            .into_iter()
            .map(|name| map.get(&name).cloned())
            .collect::<Vec<Option<CacheChannelsModel>>>();

        Ok(Some(ordered))
    }

    pub async fn find_records_by_channel_id(
        id: i64,
    ) -> Result<Option<Vec<CacheRecordsModel>>, DatabaseError> {
        let db = DB.read();
        if db.is_none() {
            return Err(DatabaseError::NotInitialized);
        }
        let db = db.as_ref().unwrap();
        let channel = CacheChannels::find()
            .filter(CacheChannelsColumn::Id.eq(&id))
            .one(db)
            .await?;
        if channel.is_none() {
            return Ok(None);
        }
        let channel = channel.unwrap();
        let records = channel.find_related(CacheRecords).all(db).await?;
        Ok(Some(records))
    }

    pub async fn insert_channel(
        active_model: CacheChannelsActiveModel,
    ) -> Result<(), DatabaseError> {
        let db = DB.read();
        if db.is_none() {
            return Err(DatabaseError::NotInitialized);
        }
        let db = db.as_ref().unwrap();
        let transaction = db.begin().await?;
        CacheChannels::insert(active_model)
            .on_conflict(
                OnConflict::columns(["id", "name"])
                    .update_columns(all_columns!(CacheChannelsColumn))
                    .to_owned(),
            )
            .exec_without_returning(&transaction)
            .await?;
        transaction.commit().await?;

        Ok(())
    }

    pub async fn insert_channels(active_models: Vec<CacheChannelsActiveModel>) -> Result<(), DatabaseError> {
        let db = DB.read();
        if db.is_none() {
            return Err(DatabaseError::NotInitialized);
        }
        let db = db.as_ref().unwrap();
        let transaction = db.begin().await?;
        CacheChannels::insert_many(active_models)
            .on_conflict(
                OnConflict::columns(["id", "name"])
                    .update_columns(all_columns!(CacheChannelsColumn))
                    .to_owned(),
            )
            .exec_without_returning(&transaction)
            .await?;
        transaction.commit().await?;

        Ok(())
    }

    pub async fn insert_record(
        active_model: CacheRecordsActiveModel,
    ) -> Result<CacheRecordsModel, DatabaseError> {
        let db = DB.read();
        if db.is_none() {
            return Err(DatabaseError::NotInitialized);
        }
        let db = db.as_ref().unwrap();
        let transaction = db.begin().await?;
        let model = active_model.insert(&transaction).await?;
        Ok(model)
    }

    pub async fn insert_records(
        active_models: Vec<CacheRecordsActiveModel>,
    ) -> Result<(), DatabaseError> {
        let db = DB.read();
        if db.is_none() {
            return Err(DatabaseError::NotInitialized);
        }
        let db = db.as_ref().unwrap();
        let transaction = db.begin().await?;
        CacheRecords::insert_many(active_models)
            .on_conflict(
                OnConflict::columns(["id", "tag", "filename"])
                    .update_columns(all_columns!(CacheRecordsColumn))
                    .to_owned(),
            )
            .exec_without_returning(&transaction)
            .await?;
        transaction.commit().await?;

        Ok(())
    }

    pub async fn remove_record_by_id(id: i64) -> Result<(), DatabaseError> {
        let db = DB.read();
        if db.is_none() {
            return Err(DatabaseError::NotInitialized);
        }
        let db = db.as_ref().unwrap();
        let transaction = db.begin().await?;
        CacheRecords::delete_many()
            .filter(CacheRecordsColumn::Id.eq(id))
            .exec(&transaction)
            .await?;
        transaction.commit().await?;

        Ok(())
    }

    pub async fn remove_records_by_ids(ids: Vec<i64>) -> Result<(), DatabaseError> {
        let db = DB.read();
        if db.is_none() {
            return Err(DatabaseError::NotInitialized);
        }
        let db = db.as_ref().unwrap();
        let transaction = db.begin().await?;
        CacheRecords::delete_many()
            .filter(CacheRecordsColumn::Id.is_in(ids))
            .exec(&transaction)
            .await?;
        transaction.commit().await?;

        Ok(())
    }
}

use crate::all_columns;
use crate::db::initializer::{DB, DatabaseError};
use crate::db::models::preclude::*;
use sea_orm::sea_query::IntoIden;
use sea_orm::sea_query::OnConflict;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, ModelTrait, QueryFilter, TransactionTrait,
};
use sea_orm::{ActiveValue, Iterable};
use std::collections::HashMap;

pub struct CookieService {}

impl CookieService {
    pub async fn find_key_by_key_id(key_id: i64) -> Result<Option<CookieKeysModel>, DatabaseError> {
        let db = DB.read().await;
        if db.is_none() {
            return Err(DatabaseError::NotInitialized);
        }
        let db = db.as_ref().unwrap();
        Ok(CookieKeys::find_by_id(key_id).one(db).await?)
    }

    pub async fn find_value_by_key_id(key_id: i64) -> Result<Option<CookiesModel>, DatabaseError> {
        let db = DB.read().await;
        if db.is_none() {
            return Err(DatabaseError::NotInitialized);
        }
        let db = db.as_ref().unwrap();
        Ok(Cookies::find_by_key_id(key_id).one(db).await?)
    }

    pub async fn find_value_by_value_id(
        value_id: i64,
    ) -> Result<Option<CookiesModel>, DatabaseError> {
        let db = DB.read().await;
        if db.is_none() {
            return Err(DatabaseError::NotInitialized);
        }
        let db = db.as_ref().unwrap();
        Ok(Cookies::find_by_id(value_id).one(db).await?)
    }

    pub async fn find_values_by_key_ids(
        key_ids: Vec<i64>,
    ) -> Result<Vec<Option<CookiesModel>>, DatabaseError> {
        let db = DB.read().await;
        if db.is_none() {
            return Err(DatabaseError::NotInitialized);
        }
        let db = db.as_ref().unwrap();

        let values = Cookies::find()
            .filter(CookiesColumn::KeyId.is_in(&key_ids))
            .all(db)
            .await?;
        let mut map = values
            .into_iter()
            .map(|value| (value.key_id.clone(), value))
            .collect::<HashMap<i64, CookiesModel>>();
        let ordered = key_ids
            .into_iter()
            .map(|key_id| map.remove(&key_id))
            .collect::<Vec<Option<CookiesModel>>>();

        Ok(ordered)
    }

    pub async fn find_keys_by_domain(
        domain: String,
    ) -> Result<Vec<CookieKeysModel>, DatabaseError> {
        let db = DB.read().await;
        if db.is_none() {
            return Err(DatabaseError::NotInitialized);
        }
        let db = db.as_ref().unwrap();
        Ok(CookieKeys::find()
            .filter(CookieKeysColumn::Domain.eq(domain))
            .all(db)
            .await?)
    }

    pub async fn find_keys_by_path(path: String) -> Result<Vec<CookieKeysModel>, DatabaseError> {
        let db = DB.read().await;
        if db.is_none() {
            return Err(DatabaseError::NotInitialized);
        }
        let db = db.as_ref().unwrap();
        Ok(CookieKeys::find()
            .filter(CookieKeysColumn::Path.eq(path))
            .all(db)
            .await?)
    }

    pub async fn find_keys_by_name(name: String) -> Result<Vec<CookieKeysModel>, DatabaseError> {
        let db = DB.read().await;
        if db.is_none() {
            return Err(DatabaseError::NotInitialized);
        }
        let db = db.as_ref().unwrap();
        Ok(CookieKeys::find()
            .filter(CookieKeysColumn::Name.eq(name))
            .all(db)
            .await?)
    }

    pub async fn find_by_key_id(
        key_id: i64,
    ) -> Result<Option<(CookieKeysModel, CookiesModel)>, DatabaseError> {
        let key = Self::find_key_by_key_id(key_id).await?;
        let value = Self::find_value_by_key_id(key_id).await?;
        if key.is_none() || value.is_none() {
            return Ok(None);
        }
        Ok(Some((key.unwrap(), value.unwrap())))
    }

    pub async fn find_by_domain(
        domain: String,
    ) -> Result<Vec<(CookieKeysModel, CookiesModel)>, DatabaseError> {
        let keys = Self::find_keys_by_domain(domain).await?;
        let key_ids = keys.iter().map(|key| key.id.clone()).collect::<Vec<i64>>();
        let values = Self::find_values_by_key_ids(key_ids)
            .await?
            .into_iter()
            .filter(|value| value.is_some())
            .map(|value| value.unwrap())
            .collect::<Vec<CookiesModel>>();
        let mut map = keys
            .into_iter()
            .map(|key| (key.id.clone(), key))
            .collect::<HashMap<i64, CookieKeysModel>>();
        let results = values
            .into_iter()
            .map(|value| (map.remove(&value.key_id).unwrap(), value))
            .collect::<Vec<(CookieKeysModel, CookiesModel)>>();

        Ok(results)
    }

    pub async fn insert(
        key: CookieKeysActiveModel,
        mut value: CookiesActiveModel,
    ) -> Result<(), DatabaseError> {
        let db = DB.read().await;
        if db.is_none() {
            return Err(DatabaseError::NotInitialized);
        }
        let db = db.as_ref().unwrap();
        let transaction = db.begin().await?;

        let path = key.path.clone().unwrap();
        let name = key.name.clone().unwrap();
        let domain = key.domain.clone().unwrap();
        let existing = CookieKeys::find_by_keys((path, name, domain))
            .one(&transaction)
            .await?;
        let id = if existing.is_none() {
            let resource = CookieKeys::insert(key)
                .on_conflict(OnConflict::new().do_nothing().to_owned())
                .exec(&transaction)
                .await?;
            resource.last_insert_id
        } else {
            existing.unwrap().id
        };
        value.key_id = ActiveValue::Set(id);

        Cookies::insert(value)
            .on_conflict(
                OnConflict::new()
                    .update_columns(all_columns!(CookiesColumn))
                    .to_owned(),
            )
            .exec_without_returning(&transaction)
            .await?;

        transaction.commit().await?;

        Ok(())
    }

    pub async fn remove_key_by_key_id(key_id: i64) -> Result<bool, DatabaseError> {
        let db = DB.read().await;
        if db.is_none() {
            return Err(DatabaseError::NotInitialized);
        }
        let db = db.as_ref().unwrap();
        let transaction = db.begin().await?;

        let resource = CookieKeys::delete_by_id(key_id).exec(&transaction).await?;

        transaction.commit().await?;

        Ok(resource.rows_affected == 1)
    }

    pub async fn clear_all() -> Result<(), DatabaseError> {
        let db = DB.read().await;
        if db.is_none() {
            return Err(DatabaseError::NotInitialized);
        }
        let db = db.as_ref().unwrap();
        let transaction = db.begin().await?;

        CookieKeys::delete_many().exec(&transaction).await?;

        transaction.commit().await?;

        Ok(())
    }
}

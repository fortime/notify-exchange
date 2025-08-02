use std::{borrow::Cow, time::Duration};

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait as _, ActiveValue, ColumnTrait as _, ConnectionTrait, DatabaseConnection,
    EntityTrait as _, IntoActiveModel as _, ModelTrait as _, QueryFilter as _, QuerySelect as _,
};
use serde::{Serialize, de::DeserializeOwned};
use uuid::Uuid;

use crate::{
    context,
    db::entity::{Cache, CacheColumn, CacheDsl, MaybeExpirable as _},
    error::{self, NotifyExchangeResult},
};

tokio::task_local! {
    static CACHE_DB_IN_TRANSACTION: bool;
}

pub enum CacheManager {
    Db(DatabaseConnection),
    Noop,
}

impl From<DatabaseConnection> for CacheManager {
    fn from(value: DatabaseConnection) -> Self {
        Self::Db(value)
    }
}

impl CacheManager {
    async fn get_from_db<Conn>(conn: &Conn, key: &str) -> NotifyExchangeResult<Option<Cache>>
    where
        Conn: ConnectionTrait,
    {
        Ok(CacheDsl::find()
            .filter(CacheColumn::Key.eq(key))
            .one(conn)
            .await?)
    }

    async fn max_id_from_db<Conn>(conn: &Conn) -> NotifyExchangeResult<i64>
    where
        Conn: ConnectionTrait,
    {
        Ok(CacheDsl::find()
            .select_only()
            .column_as(CacheColumn::Id.max(), "max")
            // If we don't use Option<i64>, it will raise an error if there is no record.
            .into_tuple::<(Option<i64>,)>()
            .one(conn)
            .await?
            .unwrap_or((None,))
            .0
            .unwrap_or(0))
    }

    fn extract_from_db_cache<T>(cache: &Cache) -> Option<T>
    where
        T: DeserializeOwned,
    {
        if cache.is_expired() {
            tracing::debug!("Cache[{}] is expired", cache.key);
            None
        } else {
            match serde_json::from_str(&cache.value) {
                Ok(v) => Some(v),
                Err(e) => {
                    tracing::error!(
                        "Unable to deserialize value: {}, key: {}, error: {e:?}",
                        cache.value,
                        cache.key
                    );
                    None
                }
            }
        }
    }

    pub async fn get<T>(&self, key: &str) -> NotifyExchangeResult<Option<T>>
    where
        T: DeserializeOwned,
    {
        match self {
            CacheManager::Db(db) => {
                let record = Self::get_from_db(db, key).await?;
                Ok(record.as_ref().and_then(Self::extract_from_db_cache))
            }
            CacheManager::Noop => Ok(None),
        }
    }

    /// It will overwrite the value if it exists, and the old value will be returned.
    pub async fn set<T>(
        &self,
        key: &str,
        v: &T,
        timeout: Option<Duration>,
    ) -> NotifyExchangeResult<Option<T>>
    where
        T: Serialize + DeserializeOwned,
    {
        match self {
            CacheManager::Db(db) => {
                context::transaction(&CACHE_DB_IN_TRANSACTION, db, async |conn| {
                    let record = Self::get_from_db(conn, key).await?;
                    let old = record.as_ref().and_then(Self::extract_from_db_cache);

                    let value = serde_json::to_string(&v)
                        .map_err(error::serde_json_error_cb("Failed to serialize key: {key}"))?;

                    let now = Utc::now();
                    if let Some(record) = record {
                        let mut record = record.into_active_model();
                        record.value = ActiveValue::Set(value);
                        record.expired_at = ActiveValue::Set(timeout.map(|t| now + t));
                        record.update(conn).await?;
                    } else {
                        let record = Cache {
                            id: Self::max_id_from_db(conn).await? + 1,
                            key: key.to_string(),
                            value,
                            created_at: now,
                            updated_at: now,
                            expired_at: timeout.map(|t| now + t),
                        };
                        record.into_active_model().insert(conn).await?;
                    }
                    Ok(old)
                })
                .await
            }
            CacheManager::Noop => Ok(None),
        }
    }

    /// It won't overwrite the value if it exists, and the value will be returned.
    pub async fn add<T, D>(&self, key: &str, v: &T, timeout: D) -> NotifyExchangeResult<bool>
    where
        T: Serialize + DeserializeOwned,
        D: Into<Option<Duration>>,
    {
        let timeout = timeout.into();
        match self {
            CacheManager::Db(db) => {
                context::transaction(&CACHE_DB_IN_TRANSACTION, db, async |conn| {
                    let record = Self::get_from_db(conn, key).await?;
                    let old: Option<T> = record.as_ref().and_then(Self::extract_from_db_cache);
                    if old.is_some() {
                        return Ok(false);
                    }

                    let value = serde_json::to_string(&v)
                        .map_err(error::serde_json_error_cb("Failed to serialize key: {key}"))?;

                    let now = Utc::now();
                    if let Some(record) = record {
                        let mut record = record.into_active_model();
                        record.value = ActiveValue::Set(value);
                        record.expired_at = ActiveValue::Set(timeout.map(|t| now + t));
                        record.update(conn).await?;
                    } else {
                        let record = Cache {
                            id: Self::max_id_from_db(conn).await? + 1,
                            key: key.to_string(),
                            value,
                            created_at: now,
                            updated_at: now,
                            expired_at: timeout.map(|t| now + t),
                        };
                        record.into_active_model().insert(conn).await?;
                    }

                    Ok(true)
                })
                .await
            }
            CacheManager::Noop => Ok(false),
        }
    }

    /// Delete a value from the cache, and return it if it existed.
    pub async fn delete<T>(&self, key: &str) -> NotifyExchangeResult<Option<T>>
    where
        T: DeserializeOwned,
    {
        match self {
            CacheManager::Db(db) => {
                context::transaction(&CACHE_DB_IN_TRANSACTION, db, async |conn| {
                    let record = Self::get_from_db(conn, key).await?;
                    let old = record.as_ref().and_then(Self::extract_from_db_cache);
                    if let Some(record) = record {
                        record.delete(conn).await?;
                    }
                    Ok(old)
                })
                .await
            }
            CacheManager::Noop => Ok(None),
        }
    }

    pub(crate) async fn add_named_token<T, D>(
        &self,
        token: &str,
        v: &T,
        timeout: D,
        retry: usize,
    ) -> NotifyExchangeResult<bool>
    where
        T: Tokenable,
        D: Into<Option<Duration>>,
    {
        let timeout = timeout.into();
        let value = serde_json::to_string(&v).map_err(error::serde_json_error_cb(format!(
            "Failed to serialize token, prefix: {}",
            T::prefix()
        )))?;
        let mut count = 0;
        let key = format!(":token:{}:{}", T::prefix(), token);
        loop {
            match self.add(&key, &value, timeout).await {
                Ok(true) => return Ok(true),
                Ok(false) => {
                    // duplicated key
                    tracing::debug!("Token exists: {key}, duplicated key");
                    return Ok(false);
                }
                Err(e) => {
                    // Treat all errors retryable
                    tracing::debug!("Failed to save token: {key}, error: {e:?}");
                }
            }
            if count == retry {
                return Err(error::overy_retry_limit(format!(
                    "Retry saving token over limit: {retry}"
                )));
            }
            count += 1;
        }
    }

    pub(crate) async fn set_named_token<T>(
        &self,
        token: &str,
        v: &T,
        timeout: Option<Duration>,
    ) -> NotifyExchangeResult<Option<T>>
    where
        T: Tokenable,
    {
        let value = serde_json::to_string(&v).map_err(error::serde_json_error_cb(format!(
            "Failed to serialize token, prefix: {}",
            T::prefix()
        )))?;
        let key = format!(":token:{}:{}", T::prefix(), token);
        let Some(old_value) = self.set(&key, &value, timeout).await? else {
            return Ok(None);
        };
        match serde_json::from_str(&old_value) {
            Ok(v) => Ok(Some(v)),
            Err(e) => {
                tracing::error!("Unable to deserialize token: {}, error: {e:?}", value);
                Ok(None)
            }
        }
    }

    /// This function provides a way to generate a token with the same format of the one returned from
    /// `save_token`.
    ///
    /// # SAFETY
    ///
    /// This token may not be unique, the caller should check if the token is unique or the value
    /// is consistent.
    pub(crate) unsafe fn new_token() -> String {
        // use v4 for security
        Uuid::new_v4().to_string()
    }

    pub(crate) async fn add_token<T, D>(
        &self,
        v: &T,
        timeout: D,
        retry: usize,
    ) -> NotifyExchangeResult<String>
    where
        T: Tokenable,
        D: Into<Option<Duration>>,
    {
        let timeout = timeout.into();
        let value = serde_json::to_string(&v).map_err(error::serde_json_error_cb(format!(
            "Failed to serialize token, prefix: {}",
            T::prefix()
        )))?;
        let mut count = 0;
        let prefix = T::prefix();
        loop {
            let token = unsafe {
                // SAFETY, only unique one will be returned
                Self::new_token()
            };
            let key = format!(":token:{}:{}", prefix, token);
            match self.add(&key, &value, timeout).await {
                Ok(true) => return Ok(token),
                Ok(false) => {
                    // duplicated key
                    tracing::debug!("Failed to save token: {key}, duplicated key");
                }
                Err(e) => {
                    // Treat all errors retryable
                    tracing::debug!("Failed to save token: {key}, error: {e:?}");
                }
            }
            if count == retry {
                return Err(error::overy_retry_limit(format!(
                    "Retry saving token over limit: {retry}"
                )));
            }
            count += 1;
        }
    }

    pub(crate) async fn get_token<T>(&self, token: &str) -> NotifyExchangeResult<Option<T>>
    where
        T: Tokenable,
    {
        let key = format!(":token:{}:{}", T::prefix(), token);
        let value = if let Some(value) = self.get::<String>(&key).await? {
            match serde_json::from_str(&value) {
                Ok(v) => Some(v),
                Err(e) => {
                    tracing::error!("Unable to deserialize token: {}, error: {e:?}", value);
                    None
                }
            }
        } else {
            None
        };
        Ok(value)
    }

    pub(crate) async fn delete_token<T>(&self, token: &str) -> NotifyExchangeResult<Option<T>>
    where
        T: Tokenable,
    {
        let key = format!(":token:{}:{}", T::prefix(), token);
        let value = if let Some(value) = self.delete::<String>(&key).await? {
            match serde_json::from_str(&value) {
                Ok(v) => Some(v),
                Err(e) => {
                    tracing::error!("Unable to deserialize token: {}, error: {e:?}", value);
                    None
                }
            }
        } else {
            None
        };
        Ok(value)
    }
}

pub(crate) trait Tokenable: Serialize + DeserializeOwned {
    /// Prefix shouldn't be equal
    fn prefix() -> Cow<'static, str>;
}

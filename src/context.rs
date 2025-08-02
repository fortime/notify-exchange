use std::sync::{
    Arc,
    atomic::{AtomicI64, Ordering},
};

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use sea_orm::{
    ColumnTrait as _, DatabaseConnection, DatabaseTransaction, EntityTrait as _, QuerySelect as _,
    TransactionTrait as _,
};
use tokio::task::LocalKey;

use crate::{
    cache::CacheManager,
    config::Config,
    db::entity::{MessageColumn, MessageDsl},
    dispatcher::DispatcherManager,
    error::{self, NotifyExchangeResult, NotifyExchangeResultExt, WwwAuthenticate},
    model::cache::SessionCache,
    service::topic::IncomingMessageSignals,
};

const DEFAULT_SECRET_SALT: [u32; 4] = [3459724393, 950097066, 3726028847, 509745701];

tokio::task_local! {
    static IN_TRANSACTION: bool;
    static MSG_DB_IN_TRANSACTION: bool;
}

#[derive(Clone)]
pub struct Context {
    shared: Arc<SharedContext>,
}

impl Context {
    pub fn new(shared: SharedContext) -> Self {
        Self {
            shared: Arc::new(shared),
        }
    }

    pub fn msg_db(&self) -> &DatabaseConnection {
        let conn = &self.shared.msg_db;
        match conn {
            #[cfg(feature = "sqlite")]
            DatabaseConnection::SqlxSqlitePoolConnection(_) => {
                // Check if it is inside a transaction. sqlite3 has at most 1 connection by default.
                // it will be blocked if it is inside a transaction.
                if MSG_DB_IN_TRANSACTION.try_with(|_| {}).is_ok() {
                    tracing::error!(
                        "You are using sqlite3, and you are getting a new connection inside a transaction, this will block the task"
                    );
                }
            }
            _ => {}
        }
        conn
    }

    pub fn db(&self) -> &DatabaseConnection {
        let conn = &self.shared.db;
        match conn {
            #[cfg(feature = "sqlite")]
            DatabaseConnection::SqlxSqlitePoolConnection(_) => {
                // Check if it is inside a transaction. sqlite3 has at most 1 connection by default.
                // it will be blocked if it is inside a transaction.
                if IN_TRANSACTION.try_with(|_| {}).is_ok() {
                    tracing::error!(
                        "You are using sqlite3, and you are getting a new connection inside a transaction, this will block the task"
                    );
                }
            }
            _ => {}
        }
        conn
    }

    pub async fn transaction<F, T>(&self, f: F) -> NotifyExchangeResult<T>
    where
        F: AsyncFnOnce(&DatabaseTransaction) -> NotifyExchangeResult<T>,
    {
        transaction(&IN_TRANSACTION, &self.shared.db, f).await
    }

    pub fn config(&self) -> &Config {
        &self.shared.config
    }

    pub fn secret_salt(&self) -> &[u8] {
        &self.shared.secret_salt
    }

    pub async fn reset_next_message_id(&self) -> NotifyExchangeResult<()> {
        let max = MessageDsl::find()
            .select_only()
            .column_as(MessageColumn::Id.max(), "max")
            .into_tuple::<(Option<i64>,)>()
            .one(self.msg_db())
            .await?
            .unwrap_or((None,))
            .0;
        self.shared
            .next_message_id
            .store(max.unwrap_or(0) + 1, Ordering::SeqCst);
        Ok(())
    }

    pub async fn next_message_id(&self) -> NotifyExchangeResult<i64> {
        let id = self.shared.next_message_id.fetch_add(1, Ordering::SeqCst);
        Ok(id)
    }

    pub fn incoming_message_signals(&self) -> &IncomingMessageSignals {
        &self.shared.incoming_message_signals
    }

    pub fn dispatcher_manager(&self) -> &DispatcherManager {
        &self.shared.dispatcher_manager
    }

    pub fn cache_manager(&self) -> &CacheManager {
        &self.shared.cache_manager
    }

    pub fn spawn_run<F, Fut>(&self, f: F)
    where
        F: FnOnce(Self) -> Fut,
        Fut: 'static + Future<Output = ()> + Send,
    {
        let context = self.clone();
        tokio::spawn(f(context));
    }
}

/// A container of all context fields so that we can put all fields in one `Arc`
pub struct SharedContext {
    db: DatabaseConnection,
    /// for saving messages in another database
    msg_db: DatabaseConnection,
    config: Config,
    secret_salt: Vec<u8>,
    next_message_id: AtomicI64,
    incoming_message_signals: IncomingMessageSignals,
    dispatcher_manager: DispatcherManager,
    cache_manager: CacheManager,
}

impl SharedContext {
    pub fn new(
        config: Config,
        db: DatabaseConnection,
        msg_db: DatabaseConnection,
        dispatcher_manager: DispatcherManager,
        cache_manager: CacheManager,
    ) -> NotifyExchangeResult<Self> {
        let secret_salt = if let Some(path) = config.secret_salt_path() {
            std::fs::read(path).whatever(format!("Failed to read salt file: {:?}", path))?
        } else {
            DEFAULT_SECRET_SALT
                .iter()
                .flat_map(|&x| x.to_le_bytes().to_vec())
                .collect::<Vec<u8>>()
        };
        Ok(Self {
            db,
            msg_db,
            config,
            secret_salt,
            dispatcher_manager,
            next_message_id: AtomicI64::new(1),
            incoming_message_signals: Default::default(),
            cache_manager,
        })
    }
}

#[derive(Clone)]
pub struct RequestContext {
    pub global: Context,
    pub user_session: Option<Arc<SessionCache>>,
}

impl RequestContext {
    pub fn new(global: Context) -> Self {
        Self {
            global,
            user_session: None,
        }
    }

    pub fn user_session(&self) -> NotifyExchangeResult<&SessionCache> {
        self.user_session.as_deref().ok_or_else(|| {
            tracing::error!("It may be some error in codes, user session is not set.");
            error::unauthorized("user session is not set", vec![WwwAuthenticate::NeLogin])
        })
    }
}

pub async fn request_context_middleware(
    State(context): State<Context>,
    mut req: Request,
    next: Next,
) -> Response {
    let request_context = RequestContext::new(context);
    req.extensions_mut().insert(request_context);
    next.run(req).await
}

pub async fn transaction<F, T>(
    scope: &'static LocalKey<bool>,
    conn: &DatabaseConnection,
    f: F,
) -> NotifyExchangeResult<T>
where
    F: AsyncFnOnce(&DatabaseTransaction) -> NotifyExchangeResult<T>,
{
    match conn {
        #[cfg(feature = "sqlite")]
        DatabaseConnection::SqlxSqlitePoolConnection(_) => {
            if scope.try_with(|_| {}).is_ok() {
                tracing::error!(
                    "You are using sqlite3, and you are starting a new transaction inside a transaction, this will block the task"
                );
            }
            scope.scope(true, inner_transaction(conn, f)).await
        }
        _ => inner_transaction(conn, f).await,
    }
}

async fn inner_transaction<F, T>(conn: &DatabaseConnection, f: F) -> NotifyExchangeResult<T>
where
    F: AsyncFnOnce(&DatabaseTransaction) -> NotifyExchangeResult<T>,
{
    let conn = conn.begin().await?;
    match f(&conn).await {
        Ok(t) => {
            conn.commit().await?;
            Ok(t)
        }
        Err(e) => {
            if let Err(e) = conn.rollback().await {
                tracing::error!("Error happened in rollback: {e:#?}");
            }
            Err(e)
        }
    }
}

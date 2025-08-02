use std::{borrow::Cow, collections::HashMap};

use bytes::Bytes;
use chrono::Utc;
use http::HttpDispatcherClient;
use sea_orm::{
    ActiveModelTrait as _, ColumnTrait as _, ConnectionTrait, DatabaseTransaction,
    EntityTrait as _, IntoActiveModel as _, Order, PaginatorTrait as _, QueryFilter as _,
    QueryOrder as _, QuerySelect as _,
    prelude::{DateTimeUtc, Expr},
    sea_query::{Cond, SimpleExpr},
};
use telegram::TelegramDispatcherClient;
use validator::Validate as _;

use crate::{
    context::Context,
    db::{
        custom_type::{Id, TransportServiceType},
        entity::{
            Endpoint, EndpointColumn, EndpointDsl, EndpointSecret, EndpointSecretColumn,
            EndpointSecretDsl, FirstPage, Secret, SecretColumn, SecretDsl, Subscription,
            TransportService, TransportServiceColumn, TransportServiceDsl, TransportServiceSecret,
            TransportServiceSecretColumn, TransportServiceSecretDsl,
        },
    },
    error::{self, NotifyExchangeResult},
    model::req::CreateEndpointRequest,
};

pub mod http;
pub mod telegram;

pub struct TransportServiceService<'a> {
    #[allow(unused)]
    context: &'a Context,
}

impl<'a> TransportServiceService<'a> {
    pub async fn list_service<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        last_service_id: Option<Id>,
        limit: usize,
    ) -> NotifyExchangeResult<(Vec<TransportService>, Option<Id>)> {
        Ok(TransportServiceDsl::find()
            .filter(
                Cond::all().add_option(last_service_id.map(|id| TransportServiceColumn::Id.lt(id))),
            )
            .order_by_desc(TransportServiceColumn::Id)
            .limit(limit as u64)
            .all(conn)
            .await
            .map(|p| FirstPage(limit).of(p))?)
    }

    pub async fn list_pagination_service<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        cond: Cond,
        order_bys: Vec<(SimpleExpr, Order)>,
        offset: usize,
        limit: usize,
    ) -> NotifyExchangeResult<Vec<TransportService>> {
        let mut query = TransportServiceDsl::find().filter(cond);
        for order_by in order_bys {
            query = query.order_by(order_by.0, order_by.1);
        }
        Ok(query
            .offset(offset as u64)
            .limit(limit as u64)
            .all(conn)
            .await?)
    }

    /// Find all enabled transport services.
    pub async fn find_all_enabled<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
    ) -> NotifyExchangeResult<Vec<TransportService>> {
        let services = TransportServiceDsl::find()
            .filter(TransportServiceColumn::Enabled.eq(true))
            .all(conn)
            .await?;
        Ok(services)
    }

    /// Find the count of enabled transport services.
    pub async fn count_enabled<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
    ) -> NotifyExchangeResult<usize> {
        let count = TransportServiceDsl::find()
            .filter(TransportServiceColumn::Enabled.eq(true))
            .count(conn)
            .await?;
        Ok(count as usize)
    }

    pub async fn insert_transport_service<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        transport_service: TransportService,
        secrets: &[(&str, Secret)],
    ) -> NotifyExchangeResult<TransportService> {
        let transport_service = transport_service.into_active_model().insert(conn).await?;
        for (secret_type, secret) in secrets {
            TransportServiceSecret {
                id: Id::new(),
                code: secret.code.clone(),
                secret_type: secret_type.to_string(),
                transport_service_id: transport_service.id,
                version: 1,
                secret_id: secret.id,
                enabled: true,
                created_at: transport_service.updated_at,
                updated_at: transport_service.updated_at,
                expired_at: None,
            }
            .into_active_model()
            .insert(conn)
            .await?;
        }
        Ok(transport_service)
    }

    pub async fn set_transport_service_enabled<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        transport_service_id: Id,
        enabled: bool,
    ) -> NotifyExchangeResult<()> {
        TransportServiceDsl::update_many()
            .filter(TransportServiceColumn::Id.eq(transport_service_id))
            .col_expr(TransportServiceColumn::Enabled, Expr::value(enabled))
            .exec(conn)
            .await?;

        // Rebuild dispatcher client
        self.context
            .dispatcher_manager()
            .refresh_by_transport_service(transport_service_id);
        Ok(())
    }

    pub async fn find_by_id<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        transport_service_id: Id,
    ) -> NotifyExchangeResult<Option<TransportService>> {
        let transport_service = TransportServiceDsl::find()
            .filter(TransportServiceColumn::Id.eq(transport_service_id))
            .one(conn)
            .await?;
        Ok(transport_service)
    }

    pub async fn find_secrets_by_transport_service_id<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        transport_service_id: Id,
        secret_type: &str,
    ) -> NotifyExchangeResult<Vec<(TransportServiceSecret, Option<Secret>)>> {
        let transport_service_secrets = TransportServiceSecretDsl::find()
            .find_also_related(SecretDsl)
            .filter(
                Cond::all()
                    .add(TransportServiceSecretColumn::TransportServiceId.eq(transport_service_id))
                    .add(TransportServiceSecretColumn::SecretType.eq(secret_type))
                    .add(TransportServiceSecretColumn::Enabled.eq(true))
                    .add(
                        Cond::any()
                            .add(TransportServiceSecretColumn::ExpiredAt.is_null())
                            .add(TransportServiceSecretColumn::ExpiredAt.gte(Utc::now())),
                    ),
            )
            .all(conn)
            .await?;
        Ok(transport_service_secrets)
    }

    /// Check whether the endpoint and subscription belong to the given transport service, and if
    /// the transport service type matches.
    fn check_relations(
        transport_service: &TransportService,
        endpoint: &Endpoint,
        subscription: &Subscription,
        transport_service_type: TransportServiceType,
    ) -> NotifyExchangeResult<()> {
        if transport_service_type != transport_service.transport_service_type {
            tracing::error!(
                "Transport service type mismatch: transport_service.transport_service_type[{:?}], transport_service_type[{:?}]",
                transport_service.transport_service_type,
                transport_service_type
            );
            return Err(error::internal_server_error(
                "Transport service type mismatch",
            ));
        }
        if transport_service.id != endpoint.transport_service_id {
            tracing::error!(
                "Transport service id mismatch: transport_service.id[{}], endpoint.transport_service_id[{}]",
                transport_service.id,
                endpoint.transport_service_id
            );
            return Err(error::internal_server_error(
                "Endpoint does not belong to the transport service",
            ));
        }
        if endpoint.id != subscription.endpoint_id {
            tracing::error!(
                "Endpoint id mismatch: endpoint.id[{}], subscription.endpoint_id[{}]",
                endpoint.id,
                subscription.endpoint_id
            );
            return Err(error::internal_server_error(
                "Subscription does not belong to the endpoint",
            ));
        }
        Ok(())
    }
}

pub struct EndpointService<'a> {
    context: &'a Context,
}

impl<'a> EndpointService<'a> {
    pub async fn insert_endpoints<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        user_id: Id,
        create_endpoint_requests: Vec<CreateEndpointRequest<'_>>,
    ) -> NotifyExchangeResult<Vec<Endpoint>> {
        let now = Utc::now();
        let mut res = Vec::with_capacity(create_endpoint_requests.len());
        for req in create_endpoint_requests {
            req.validate()?;
            res.push(
                Endpoint {
                    id: Id::new(),
                    code: req.code.into_owned(),
                    name: req.name.into_owned(),
                    transport_service_id: req.transport_service_id,
                    transport_service_type: req.transport_service_type,
                    user_id,
                    options: req.options,
                    description: req.description.map(Cow::into_owned),
                    is_public: req.is_public,
                    created_at: now,
                    updated_at: now,
                }
                .into_active_model()
                .insert(conn)
                .await?,
            );
        }
        Ok(res)
    }

    pub async fn find_by_uniq_key<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        code: &str,
        transport_service_id: Id,
    ) -> NotifyExchangeResult<Option<Endpoint>> {
        Ok(EndpointDsl::find()
            .filter(EndpointColumn::Code.eq(code))
            .filter(EndpointColumn::TransportServiceId.eq(transport_service_id))
            .one(conn)
            .await?)
    }

    pub async fn find_endpoint_and_secrets_by_id<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        endpoint_id: Id,
        secret_type: &str,
        code: Option<&str>,
    ) -> NotifyExchangeResult<Option<(Endpoint, Vec<(EndpointSecret, Option<Secret>)>)>> {
        let endpoint = EndpointDsl::find_by_id(endpoint_id).one(conn).await?;
        if let Some(e) = endpoint {
            Ok(Some((
                e,
                self.find_secrets_by_id(conn, endpoint_id, secret_type, code, Some(true), false)
                    .await?,
            )))
        } else {
            Ok(None)
        }
    }

    pub async fn find_secrets_by_id<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        endpoint_id: Id,
        secret_type: &str,
        code: Option<&str>,
        enabled: Option<bool>,
        allow_expired: bool,
    ) -> NotifyExchangeResult<Vec<(EndpointSecret, Option<Secret>)>> {
        let endpoint_secrets = EndpointSecretDsl::find()
            .find_also_related(SecretDsl)
            .filter(
                Cond::all()
                    .add(EndpointSecretColumn::EndpointId.eq(endpoint_id))
                    .add(EndpointSecretColumn::SecretType.eq(secret_type))
                    .add_option(enabled.map(|b| EndpointSecretColumn::Enabled.eq(b)))
                    .add_option(code.map(|c| EndpointSecretColumn::Code.eq(c)))
                    .add_option(allow_expired.then(|| {
                        Cond::any()
                            .add(EndpointSecretColumn::ExpiredAt.is_null())
                            .add(EndpointSecretColumn::ExpiredAt.gte(Utc::now()))
                    })),
            )
            .order_by_asc(EndpointSecretColumn::CreatedAt)
            .all(conn)
            .await?;
        Ok(endpoint_secrets)
    }

    pub async fn find_aggregated_enabled_secrets_by_id<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        endpoint_id: Id,
    ) -> NotifyExchangeResult<Vec<EndpointSecret>> {
        let endpoint_secrets = EndpointSecretDsl::find()
            .filter(
                Cond::all()
                    .add(EndpointSecretColumn::EndpointId.eq(endpoint_id))
                    .add(EndpointSecretColumn::Enabled.eq(true))
                    .add(
                        Cond::any()
                            .add(EndpointSecretColumn::ExpiredAt.is_null())
                            .add(EndpointSecretColumn::ExpiredAt.gte(Utc::now())),
                    ),
            )
            .group_by(EndpointSecretColumn::EndpointId)
            .group_by(EndpointSecretColumn::Code)
            // Use latest one
            .order_by_desc(EndpointSecretColumn::CreatedAt)
            .all(conn)
            .await?;
        Ok(endpoint_secrets)
    }

    pub async fn count_endpoint_secrets_group_by_type<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        endpoint_id: Id,
        secret_type: &str,
    ) -> NotifyExchangeResult<u64> {
        Ok(EndpointSecretDsl::find()
            .filter(
                Cond::all()
                    .add(EndpointSecretColumn::EndpointId.eq(endpoint_id))
                    .add(EndpointSecretColumn::SecretType.eq(secret_type)),
            )
            .group_by(EndpointSecretColumn::EndpointId)
            .group_by(EndpointSecretColumn::SecretType)
            .count(conn)
            .await?)
    }

    pub async fn count_endpoint_secrets_group_by_code<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        endpoint_id: Id,
        code: &str,
    ) -> NotifyExchangeResult<u64> {
        Ok(EndpointSecretDsl::find()
            .filter(
                Cond::all()
                    .add(EndpointSecretColumn::EndpointId.eq(endpoint_id))
                    .add(EndpointSecretColumn::Code.eq(code)),
            )
            .group_by(EndpointSecretColumn::EndpointId)
            .group_by(EndpointSecretColumn::Code)
            .count(conn)
            .await?)
    }

    pub async fn decrypt_endpoint_secrets<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        endpoint_secrets: Vec<(EndpointSecret, Option<Secret>)>,
    ) -> NotifyExchangeResult<Vec<(EndpointSecret, Bytes)>> {
        let mut res = vec![];
        let secret_service = self.context.secret_service();
        for (endpoint_secret, secret) in endpoint_secrets {
            let Some(secret) = secret else {
                tracing::error!(
                    "Endpoint secret does not exist: {}/{}",
                    endpoint_secret.endpoint_id,
                    endpoint_secret.code
                );
                continue;
            };
            res.push((
                endpoint_secret,
                secret_service.decrypt_secret(conn, &secret).await?.into(),
            ));
        }
        Ok(res)
    }

    pub async fn find_by_id<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        endpoint_id: Id,
    ) -> NotifyExchangeResult<Option<Endpoint>> {
        let endpoint = EndpointDsl::find()
            .filter(EndpointColumn::Id.eq(endpoint_id))
            .one(conn)
            .await?;
        Ok(endpoint)
    }

    pub async fn find_endpoint_secret_by_id<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        endpoint_secret_id: Id,
    ) -> NotifyExchangeResult<Option<(EndpointSecret, Option<Secret>)>> {
        let endpoint_secret = EndpointSecretDsl::find()
            .find_also_related(SecretDsl)
            .filter(Cond::all().add(EndpointSecretColumn::Id.eq(endpoint_secret_id)))
            .one(conn)
            .await?;
        Ok(endpoint_secret)
    }

    async fn list_endpoint<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        cond: Cond,
        last_endpoint_id: Option<Id>,
        limit: usize,
    ) -> NotifyExchangeResult<(Vec<Endpoint>, Option<Id>)> {
        Ok(EndpointDsl::find()
            .filter(cond.add_option(last_endpoint_id.map(|id| EndpointColumn::Id.lt(id))))
            .order_by_desc(EndpointColumn::Id)
            .limit(limit as u64)
            .all(conn)
            .await
            .map(|p| FirstPage(limit).of(p))?)
    }

    pub async fn list_pagination_endpoint<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        cond: Cond,
        order_bys: Vec<(SimpleExpr, Order)>,
        offset: usize,
        limit: usize,
    ) -> NotifyExchangeResult<Vec<Endpoint>> {
        let mut query = EndpointDsl::find().filter(cond);
        for order_by in order_bys {
            query = query.order_by(order_by.0, order_by.1);
        }
        Ok(query
            .offset(offset as u64)
            .limit(limit as u64)
            .all(conn)
            .await?)
    }

    async fn list_endpoint_secret<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        endpoint_id: Id,
        cond: Cond,
        last_endpoint_secret_id: Option<Id>,
        limit: usize,
    ) -> NotifyExchangeResult<(Vec<EndpointSecret>, Option<Id>)> {
        Ok(EndpointSecretDsl::find()
            .filter(
                cond.add(EndpointSecretColumn::EndpointId.eq(endpoint_id))
                    .add_option(last_endpoint_secret_id.map(|id| EndpointSecretColumn::Id.lt(id))),
            )
            .order_by_desc(EndpointSecretColumn::Id)
            .limit(limit as u64)
            .all(conn)
            .await
            .map(|p| FirstPage(limit).of(p))?)
    }

    /// If `version` is bigger than 10, it will remove those which version is smaller than `version - 4`, and than the
    /// version of all remaining records will subtract `version - 4`.
    #[allow(clippy::too_many_arguments)]
    async fn insert_endpoint_secret(
        &self,
        conn: &DatabaseTransaction,
        endpoint_id: Id,
        code: &str,
        version: i8,
        secret_type: &str,
        secret: &[u8],
        enabled: bool,
        expired_at: Option<DateTimeUtc>,
    ) -> NotifyExchangeResult<EndpointSecret> {
        let endpoint_secret_id = Id::new();

        let secret = self
            .context
            .secret_service()
            .insert_secret(
                conn,
                &endpoint_secret_id.to_string(),
                secret,
                None,
                Some(format!(
                    "endpoint secret for: {endpoint_id}-{code}-{version}"
                )),
            )
            .await?;

        let version = if version > 10 {
            let min_version = version - 4;
            let deleted_count = self
                .delete_endpoint_secrets(
                    conn,
                    Cond::all()
                        .add(EndpointSecretColumn::EndpointId.eq(endpoint_id))
                        .add(EndpointSecretColumn::Code.eq(code))
                        .add(EndpointSecretColumn::SecretType.eq(secret_type))
                        .add(EndpointSecretColumn::Version.lt(min_version)),
                )
                .await?;
            tracing::info!(
                "Delete endpoint secrets[{endpoint_id}|{code}|{secret_type}] version less than {min_version}: {deleted_count}"
            );
            let updated_count = EndpointSecretDsl::update_many()
                .filter(
                    Cond::all()
                        .add(EndpointSecretColumn::EndpointId.eq(endpoint_id))
                        .add(EndpointSecretColumn::Code.eq(code))
                        .add(EndpointSecretColumn::SecretType.eq(secret_type))
                        .add(EndpointSecretColumn::Version.gte(min_version)),
                )
                .col_expr(
                    EndpointSecretColumn::Version,
                    Expr::col(EndpointSecretColumn::Version).sub(min_version),
                )
                .exec(conn)
                .await?
                .rows_affected;
            tracing::info!(
                "Update endpoint secrets[{endpoint_id}|{code}|{secret_type}] version to subtract {min_version}: {updated_count}"
            );
            version - min_version
        } else {
            version
        };

        let now = Utc::now();
        let endpoint_secret = EndpointSecret {
            id: endpoint_secret_id,
            code: code.to_string(),
            secret_type: secret_type.to_string(),
            endpoint_id,
            version,
            secret_id: secret.id,
            enabled,
            created_at: now,
            updated_at: now,
            expired_at,
        }
        .into_active_model()
        .insert(conn)
        .await?;
        Ok(endpoint_secret)
    }

    #[allow(clippy::too_many_arguments)]
    async fn update_endpoint_secret(
        &self,
        conn: &DatabaseTransaction,
        endpoint_id: Id,
        code: &str,
        secret_type: &str,
        secret: &[u8],
        enabled: bool,
        expired_at: Option<DateTimeUtc>,
    ) -> NotifyExchangeResult<EndpointSecret> {
        let version = EndpointSecretDsl::find()
            .select_only()
            .filter(
                Cond::all().add(
                    EndpointSecretColumn::EndpointId
                        .eq(endpoint_id)
                        .add(EndpointSecretColumn::Code.eq(code)),
                ),
            )
            .column_as(EndpointSecretColumn::Version.max(), "max")
            .into_tuple::<(Option<i8>,)>()
            .one(conn)
            .await?
            .unwrap_or((None,))
            .0;
        let Some(version) = version else {
            return Err(error::not_found("Endpoint secret does not exist"));
        };
        let new_version = version.checked_add(1).unwrap_or(0);

        self.insert_endpoint_secret(
            conn,
            endpoint_id,
            code,
            new_version,
            secret_type,
            secret,
            enabled,
            expired_at,
        )
        .await
    }

    pub async fn lock_endpoint(
        &self,
        conn: &DatabaseTransaction,
        endpoint_id: Id,
        exclusive: bool,
    ) -> NotifyExchangeResult<Option<Endpoint>> {
        if exclusive {
            Ok(EndpointDsl::find_by_id(endpoint_id)
                .lock_exclusive()
                .one(conn)
                .await?)
        } else {
            Ok(EndpointDsl::find_by_id(endpoint_id)
                .lock_shared()
                .one(conn)
                .await?)
        }
    }

    async fn delete_endpoint_secrets(
        &self,
        conn: &DatabaseTransaction,
        cond: Cond,
    ) -> NotifyExchangeResult<u64> {
        let secret_ids: Vec<Id> = EndpointSecretDsl::find()
            .filter(cond.clone())
            .select_only()
            .column(EndpointSecretColumn::SecretId)
            .into_tuple()
            .all(conn)
            .await?;

        let count = secret_ids.len() as u64;

        // delete related secret
        SecretDsl::delete_many()
            .filter(Cond::all().add(SecretColumn::Id.is_in(secret_ids)))
            .exec(conn)
            .await?;

        EndpointSecretDsl::delete_many()
            .filter(cond)
            .exec(conn)
            .await?;
        Ok(count)
    }

    pub async fn delete_endpoint_secret_by_code_and_secret_type(
        &self,
        conn: &DatabaseTransaction,
        endpoint_id: Id,
        code: &str,
        secret_type: &str,
    ) -> NotifyExchangeResult<u64> {
        // Lock the endpoint, so the dispatcher_manager can have the latest record.
        self.lock_endpoint(conn, endpoint_id, true).await?;

        let count = self
            .delete_endpoint_secrets(
                conn,
                Cond::all()
                    .add(EndpointSecretColumn::EndpointId.eq(endpoint_id))
                    .add(EndpointSecretColumn::Code.eq(code))
                    .add(EndpointSecretColumn::SecretType.eq(secret_type)),
            )
            .await?;

        // Rebuild dispatcher client
        self.context
            .dispatcher_manager()
            .refresh_by_endpoint(endpoint_id);

        Ok(count)
    }

    pub async fn delete_by_id(
        &self,
        conn: &DatabaseTransaction,
        endpoint_id: Id,
    ) -> NotifyExchangeResult<()> {
        // Lock the endpoint, so the dispatcher_manager can have the latest record.
        let Some(endpoint) = self.lock_endpoint(conn, endpoint_id, true).await? else {
            return Ok(());
        };

        // There may be some cleanup work for different transport services
        self.context
            .transport_service_operator()
            .check_endpoint_before_deletion(conn, &endpoint)
            .await?;

        // It will unload dispatcher client of all subscriptions
        self.context
            .subscription_service()
            .delete_by_endpoint_id(conn, endpoint_id)
            .await?;

        // Delete secrets
        let endpoint_secrets = EndpointSecretDsl::find()
            .filter(Cond::all().add(EndpointSecretColumn::EndpointId.eq(endpoint_id)))
            .all(conn)
            .await?;

        // Delete related secret
        SecretDsl::delete_many()
            .filter(
                Cond::all()
                    .add(SecretColumn::Id.is_in(endpoint_secrets.iter().map(|es| es.secret_id))),
            )
            .exec(conn)
            .await?;

        EndpointSecretDsl::delete_many()
            .filter(Cond::all().add(EndpointSecretColumn::EndpointId.eq(endpoint_id)))
            .exec(conn)
            .await?;

        // Delete endpoint
        EndpointDsl::delete_by_id(endpoint_id).exec(conn).await?;

        Ok(())
    }

    pub async fn find_by_ids<Conn: ConnectionTrait, I>(
        &self,
        conn: &Conn,
        endpoint_ids: I,
    ) -> NotifyExchangeResult<HashMap<Id, Endpoint>>
    where
        I: IntoIterator<Item = Id>,
    {
        Ok(EndpointDsl::find()
            .filter(Cond::all().add(EndpointColumn::Id.is_in(endpoint_ids)))
            .all(conn)
            .await?
            .into_iter()
            .map(|e| (e.id, e))
            .collect())
    }
}

pub struct TransportServiceOperator<'a> {
    context: &'a Context,
}

macro_rules! dispatch_transport_service_op {
    ($type:expr, $context:expr, $fn:ident, $($param:ident),*) => {
        match $type {
            $crate::db::custom_type::TransportServiceType::Telegram => {
                $crate::service::transport::telegram::TelegramService::new($context).$fn($($param),*).await
            },
            $crate::db::custom_type::TransportServiceType::Http => {
                $crate::service::transport::http::HttpService::new($context).$fn($($param),*).await
            },
        }
    };
}

impl<'a> TransportServiceOperator<'a> {
    pub async fn start<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        transport_service: &TransportService,
    ) -> NotifyExchangeResult<()> {
        dispatch_transport_service_op!(
            transport_service.transport_service_type,
            self.context,
            start,
            conn,
            transport_service
        )
    }

    pub async fn stop<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        transport_service: &TransportService,
    ) -> NotifyExchangeResult<()> {
        dispatch_transport_service_op!(
            transport_service.transport_service_type,
            self.context,
            stop,
            conn,
            transport_service
        )
    }

    pub async fn check_endpoint_subscribable<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        endpoint: &Endpoint,
    ) -> NotifyExchangeResult<()> {
        dispatch_transport_service_op!(
            endpoint.transport_service_type,
            self.context,
            check_endpoint_subscribable,
            conn,
            endpoint
        )
    }

    pub async fn check_endpoint_before_deletion(
        &self,
        conn: &DatabaseTransaction,
        endpoint: &Endpoint,
    ) -> NotifyExchangeResult<()> {
        dispatch_transport_service_op!(
            endpoint.transport_service_type,
            self.context,
            check_endpoint_before_deletion,
            conn,
            endpoint
        )
    }

    pub async fn build_telegram_dispatcher_client(
        context: &Context,
        transport_service: &TransportService,
        endpoint: &Endpoint,
        subscription: &Subscription,
    ) -> NotifyExchangeResult<Option<TelegramDispatcherClient>> {
        TelegramDispatcherClient::new(context, transport_service, endpoint, subscription).await
    }

    pub async fn build_http_dispatcher_client(
        context: &Context,
        transport_service: &TransportService,
        endpoint: &Endpoint,
        subscription: &Subscription,
    ) -> NotifyExchangeResult<Option<HttpDispatcherClient>> {
        HttpDispatcherClient::new(context, transport_service, endpoint, subscription).await
    }
}

impl Context {
    pub fn transport_service_service(&self) -> TransportServiceService<'_> {
        TransportServiceService { context: self }
    }

    pub fn endpoint_service(&self) -> EndpointService<'_> {
        EndpointService { context: self }
    }

    pub fn transport_service_operator(&self) -> TransportServiceOperator<'_> {
        TransportServiceOperator { context: self }
    }
}

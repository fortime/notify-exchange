use std::{collections::HashSet, sync::Arc, time::Duration};

use axum::{
    Extension, Json, Router,
    extract::{Path, Query},
    middleware, routing,
};
use either::Either;
use sea_orm::{IntoSimpleExpr as _, Order, prelude::DateTimeUtc, sea_query::Cond};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::{
    auth::{self},
    context::RequestContext,
    controller::{MaybeSuccessResponse, PaginationQuery},
    db::{
        custom_type::Id,
        entity::{Role, TransportServiceColumn},
    },
    error::{self, NotifyExchangeError},
    model::{
        cache::TelegramSetupCache,
        req::{CreateHttpTransportServiceRequest, CreateUserRequest},
    },
    service::transport::{http::HttpService, telegram},
};

const SETUP_TIMEOUT: Duration = Duration::from_secs(60 * 5);

pub fn route<S>(router: Router<S>) -> Router<S>
where
    S: 'static + Clone + Sync + Send,
{
    let sub_router = Router::<S>::new()
        .route("/", routing::get(get_transport_services))
        .route(
            "/{:transport_service_id}/enabled",
            routing::put(set_transport_service_enabled),
        )
        .route("/http", routing::post(create_http_transport_service))
        .route(
            "/telegram",
            routing::post(create_telegram_transport_service),
        )
        .route("/telegram/setup", routing::get(get_telegram_setup_status))
        .route_layer(middleware::from_fn_with_state(
            Arc::new(HashSet::from([Role::ADMIN])),
            auth::check_user_role_middleware,
        ))
        .route_layer(middleware::from_fn(auth::web_session_auth_middleware));

    router.nest("/v1/admin/transport-service", sub_router)
}

#[derive(Serialize)]
struct TransportServiceInfo {
    id: Id,
    name: String,
    description: Option<String>,
    transport_service_type: u16,
    enabled: bool,
    created_at: DateTimeUtc,
}

#[derive(Serialize)]
struct GetTransportServicesResponse {
    transport_services: Vec<TransportServiceInfo>,
}

#[derive(Deserialize, Validate)]
struct CreateTelegramTransportServiceRequest {
    #[validate(length(min = 4, max = 128))]
    pub name: String,
    #[validate(length(max = 1024))]
    pub description: Option<String>,
    #[validate(length(max = 1024))]
    pub token: String,
    pub user_id: Option<Id>,
    pub new_user: Option<CreateUserRequest<'static>>,
}

#[derive(Serialize)]
struct CreateTelegramTransportServiceResponse {
    pub setup_token: String,
}

#[derive(Deserialize)]
struct GetTelegramSetupRequest {
    token: String,
}

#[derive(Serialize)]
struct GetTelegramSetupResponse {
    created: Option<bool>,
}

async fn get_transport_services(
    Extension(req_ctx): Extension<RequestContext>,
    Query(pagination): Query<PaginationQuery>,
) -> MaybeSuccessResponse<GetTransportServicesResponse> {
    pagination.validate()?;
    let context = &req_ctx.global;

    let services = context
        .transport_service_service()
        .list_pagination_service(
            context.db(),
            Cond::all(),
            vec![(
                TransportServiceColumn::CreatedAt.into_simple_expr(),
                Order::Desc,
            )],
            pagination.offset(),
            pagination.size,
        )
        .await?;

    Ok(GetTransportServicesResponse {
        transport_services: services
            .into_iter()
            .map(|s| TransportServiceInfo {
                id: s.id,
                name: s.name,
                description: s.description,
                transport_service_type: s.transport_service_type as u16,
                enabled: s.enabled,
                created_at: s.created_at,
            })
            .collect(),
    }
    .into())
}

async fn set_transport_service_enabled(
    Extension(req_ctx): Extension<RequestContext>,
    Path(transport_service_id): Path<Id>,
    Json(enabled): Json<bool>,
) -> MaybeSuccessResponse<()> {
    let context = &req_ctx.global;

    context
        .transport_service_service()
        .set_transport_service_enabled(context.db(), transport_service_id, enabled)
        .await?;

    Ok(().into())
}

async fn create_http_transport_service(
    Extension(req_ctx): Extension<RequestContext>,
    Json(req): Json<CreateHttpTransportServiceRequest<'_>>,
) -> MaybeSuccessResponse<Id> {
    let context = &req_ctx.global;
    let user = &req_ctx.user_session()?.user;
    let name = req.name.to_string();

    let service = context
        .transaction(async |conn| {
            HttpService::new(context)
                .create_transport_service(conn, user, req)
                .await
        })
        .await?;

    if service.name != name {
        return Err(error::conflict("HTTP transport service already exists"));
    }

    Ok(service.id.into())
}

async fn create_telegram_transport_service(
    Extension(req_ctx): Extension<RequestContext>,
    Json(req): Json<CreateTelegramTransportServiceRequest>,
) -> MaybeSuccessResponse<CreateTelegramTransportServiceResponse> {
    req.validate()?;

    let bot = match telegram::init_bot(&req.token).await {
        Ok(b) => b,
        Err(NotifyExchangeError::TelegramRequest { source, .. }) => {
            tracing::warn!("Failed to initialize Telegram bot: {source:?}");
            return Err(error::invalid_request(
                "Invalid Telegram bot token".to_string(),
            ));
        }
        Err(e) => return Err(e),
    };

    let context = &req_ctx.global;

    let user = &req_ctx.user_session()?.user;

    let setup_token = context
        .transaction(async |conn| {
            let user_service = context.user_service();

            let (maybe_user_id, setup_token) = if let Some(user_id) = req.user_id {
                // Ensure user exists
                let _ = user_service.find_user_by_id(conn, user_id).await?;
                (Either::Right(user_id), Uuid::new_v4().to_string())
            } else if let Some(new_user) = req.new_user {
                let pending_user = user_service
                    .create_pending_user(conn, new_user, Default::default(), SETUP_TIMEOUT)
                    .await?;
                (Either::Left(pending_user.id), pending_user.id.to_string())
            } else {
                (Either::Right(user.id), Uuid::new_v4().to_string())
            };

            if !context
                .cache_manager()
                .add_named_token(
                    &setup_token,
                    &TelegramSetupCache { created: false },
                    Some(SETUP_TIMEOUT),
                    3,
                )
                .await?
            {
                return Err(error::internal_server_error(
                    "Duplicated telegram setup token".to_string(),
                ));
            }

            tokio::spawn({
                let context = context.clone();
                let setup_token = setup_token.clone();
                async move {
                    if let Err(e) = telegram::wait_and_finish_setup(
                        context,
                        bot,
                        req.name,
                        req.description,
                        maybe_user_id,
                        setup_token,
                        SETUP_TIMEOUT,
                    )
                    .await
                    {
                        tracing::error!("wait_and_finish_setup error: {e:#?}");
                    }
                }
            });

            Ok(setup_token)
        })
        .await?;

    Ok(CreateTelegramTransportServiceResponse { setup_token }.into())
}

async fn get_telegram_setup_status(
    Extension(req_ctx): Extension<RequestContext>,
    Query(req): Query<GetTelegramSetupRequest>,
) -> MaybeSuccessResponse<GetTelegramSetupResponse> {
    let context = &req_ctx.global;
    let cache: Option<TelegramSetupCache> = context.cache_manager().get_token(&req.token).await?;

    let created = cache.map(|c| c.created);
    Ok(GetTelegramSetupResponse { created }.into())
}

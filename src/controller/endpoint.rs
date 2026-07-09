use std::{borrow::Cow, collections::HashMap};

use axum::{
    Extension, Json, Router,
    extract::{Path, Query},
    middleware, routing,
};
use axum_typed_multipart::{TryFromMultipart, TypedMultipart};
use base64::{Engine as _, prelude::BASE64_STANDARD};
use bytes::Bytes;
use sea_orm::{
    ColumnTrait as _, IntoSimpleExpr as _, Order, prelude::DateTimeUtc, sea_query::Cond,
};
use serde::{Deserialize, Serialize};
use validator::Validate;

use crate::{
    auth,
    context::{Context, RequestContext},
    controller::{self, MaybeSuccessResponse, PaginationQuery},
    db::{
        custom_type::{Id, TransportServiceType},
        entity::{Endpoint, EndpointColumn, SubscriptionColumn, TransportServiceColumn, User},
    },
    error::{self, NotifyExchangeResult},
    service::transport::http::HttpService,
};

pub fn route<S>(router: Router<S>) -> Router<S>
where
    S: 'static + Clone + Sync + Send,
{
    let sub_router = Router::<S>::new()
        .route("/", routing::get(get_endpoints))
        .route("/http", routing::post(create_http_endpoint))
        .route(
            "/http-transport-service",
            routing::get(get_http_transport_services),
        )
        .route(
            "/{:endpoint_id}",
            routing::get(get_endpoint_info).delete(delete_endpoint),
        )
        .route(
            "/{:endpoint_id}/secret",
            routing::get(get_endpoint_secrets)
                .post(create_endpoint_secret)
                .put(update_endpoint_secret)
                .delete(delete_endpoint_secret),
        )
        .route(
            "/{:endpoint_id}/secret/{:endpoint_secret_id}/masked",
            routing::get(get_masked_endpoint_secret),
        )
        .route(
            "/{:endpoint_id}/subscription",
            routing::get(get_endpoint_subscriptions),
        )
        .route(
            "/{:endpoint_id}/subscription/{:subscription_id}",
            routing::delete(delete_endpoint_subscription),
        )
        .route_layer(middleware::from_fn(auth::web_session_auth_middleware));
    router.nest("/v1/endpoint", sub_router)
}

#[derive(Serialize)]
struct EndpointInfo {
    id: Id,
    name: String,
    code: String,
    transport_service_type: u16,
    created_at: DateTimeUtc,
}

#[derive(Serialize)]
struct GetEndpointsResponse {
    endpoints: Vec<EndpointInfo>,
}

#[derive(Serialize)]
struct GetEndpointInfoResponse {
    id: Id,
    name: String,
    code: String,
    transport_service_id: Id,
    transport_service_type: u16,
    owner_id: Id,
    description: Option<String>,
    is_public: bool,
    options: String,
    created_at: DateTimeUtc,
    updated_at: DateTimeUtc,
    transport_service_name: String,
    owner_name: String,
}

#[derive(Serialize)]
struct HttpTransportServiceInfo {
    id: Id,
    name: String,
    description: Option<String>,
}

#[derive(Serialize)]
struct GetHttpTransportServicesResponse {
    transport_services: Vec<HttpTransportServiceInfo>,
}

#[derive(Deserialize, Validate)]
struct CreateHttpEndpointRequest {
    pub transport_service_id: Id,
    #[validate(length(min = 4, max = 128))]
    pub name: String,
    #[validate(length(min = 4, max = 128), regex(path = *crate::model::CODE_CHARS))]
    pub code: String,
    #[validate(length(max = 1024))]
    pub description: Option<String>,
}

#[derive(Serialize)]
struct CreateHttpEndpointResponse {
    id: Id,
}

#[derive(Serialize)]
struct EndpointSecretInfo {
    id: Id,
    code: String,
    endpoint_id: Id,
    secret_type: String,
    created_at: DateTimeUtc,
    updated_at: DateTimeUtc,
    expired_at: Option<DateTimeUtc>,
    revertable: bool,
}

#[derive(Serialize)]
struct GetEndpointSecretsResponse {
    secrets: Vec<EndpointSecretInfo>,
}

#[derive(TryFromMultipart, Validate)]
struct CreateEndpointSecretRequest {
    #[validate(length(min = 4, max = 64), regex(path = *crate::model::CODE_CHARS))]
    #[form_data(limit = "64B")]
    code: String,
    #[validate(length(min = 4, max = 32), regex(path = *crate::model::CODE_CHARS))]
    #[form_data(limit = "32B")]
    secret_type: String,
    /// The content of the secret, has higher priority over `base64_secret`, and it can be longer
    /// than `base64_secret`
    #[form_data(limit = "20KiB")]
    secret: Option<Bytes>,
    #[form_data(limit = "20KiB")]
    /// The content of the secret encoded in base64.
    base64_secret: Option<String>,
}

#[derive(Deserialize, Validate)]
struct EndpointSecretQuery {
    #[validate(length(min = 4, max = 64), regex(path = *crate::model::CODE_CHARS))]
    code: String,
    #[validate(length(min = 4, max = 32), regex(path = *crate::model::CODE_CHARS))]
    secret_type: String,
}

#[derive(TryFromMultipart)]
struct UpdateEndpointSecretRequest {
    /// The content of the secret, has higher priority over `base64_secret`, and it can be longer
    /// than `base64_secret`
    #[form_data(limit = "20KiB")]
    secret: Option<Bytes>,
    #[form_data(limit = "20KiB")]
    /// The content of the secret encoded in base64.
    base64_secret: Option<String>,
}

#[derive(Serialize)]
struct CreateEndpointSecretResponse {
    base64_secret: Option<String>,
}

#[derive(Serialize)]
struct GetMaskedEndpointSecretResponse {
    masked_secret: Option<String>,
}

#[derive(Serialize)]
struct EndpointSubscriptionInfo {
    id: Id,
    topic_id: Id,
    topic_name: String,
    topic_code: String,
    committed_offset: i64,
    latest_offset: i64,
    created_at: DateTimeUtc,
}

#[derive(Serialize)]
struct GetEndpointSubscriptionsResponse {
    subscriptions: Vec<EndpointSubscriptionInfo>,
}

async fn get_endpoint_and_assure_user(
    context: &Context,
    endpoint_id: Id,
    user: &User,
) -> NotifyExchangeResult<Endpoint> {
    let endpoint = context
        .endpoint_service()
        .find_by_id(context.db(), endpoint_id)
        .await?
        .ok_or_else(|| error::not_found("Endpoint not found"))?;
    controller::assure_user(user.id, endpoint.user_id)?;
    Ok(endpoint)
}

async fn get_endpoints(
    Extension(req_ctx): Extension<RequestContext>,
    Query(pagination): Query<PaginationQuery>,
) -> MaybeSuccessResponse<GetEndpointsResponse> {
    pagination.validate()?;
    let context = &req_ctx.global;

    let user = &req_ctx.user_session()?.user;

    let endpoints = context
        .endpoint_service()
        .list_pagination_endpoint(
            context.db(),
            Cond::all().add(EndpointColumn::UserId.eq(user.id)),
            vec![(EndpointColumn::CreatedAt.into_simple_expr(), Order::Desc)],
            pagination.offset(),
            pagination.size,
        )
        .await?;

    Ok(GetEndpointsResponse {
        endpoints: endpoints
            .into_iter()
            .map(|e| EndpointInfo {
                id: e.id,
                name: e.name,
                code: e.code,
                transport_service_type: e.transport_service_type as u16,
                created_at: e.created_at,
            })
            .collect(),
    }
    .into())
}

async fn get_endpoint_info(
    Extension(req_ctx): Extension<RequestContext>,
    Path(endpoint_id): Path<Id>,
) -> MaybeSuccessResponse<GetEndpointInfoResponse> {
    let context = &req_ctx.global;

    let user = &req_ctx.user_session()?.user;
    let endpoint = get_endpoint_and_assure_user(context, endpoint_id, user).await?;

    let transport_service = context
        .transport_service_service()
        .find_by_id(context.db(), endpoint.transport_service_id)
        .await?;

    Ok(GetEndpointInfoResponse {
        id: endpoint.id,
        name: endpoint.name,
        code: endpoint.code,
        transport_service_id: endpoint.transport_service_id,
        transport_service_type: endpoint.transport_service_type as u16,
        owner_id: endpoint.user_id,
        description: endpoint.description,
        is_public: endpoint.is_public,
        options: endpoint.options,
        created_at: endpoint.created_at,
        updated_at: endpoint.updated_at,
        owner_name: user.username.clone(),
        transport_service_name: transport_service
            .as_ref()
            .map(|ts| ts.name.clone())
            .unwrap_or_else(|| "DELETED".to_string()),
    }
    .into())
}

async fn get_http_transport_services(
    Extension(req_ctx): Extension<RequestContext>,
    Query(pagination): Query<PaginationQuery>,
) -> MaybeSuccessResponse<GetHttpTransportServicesResponse> {
    pagination.validate()?;
    let context = &req_ctx.global;

    let transport_services = context
        .transport_service_service()
        .list_pagination_service(
            context.db(),
            Cond::all()
                .add(TransportServiceColumn::TransportServiceType.eq(TransportServiceType::Http))
                .add(TransportServiceColumn::Enabled.eq(true)),
            vec![(
                TransportServiceColumn::CreatedAt.into_simple_expr(),
                Order::Desc,
            )],
            (pagination.page - 1) * pagination.size,
            pagination.size,
        )
        .await?
        .into_iter()
        .map(|s| HttpTransportServiceInfo {
            id: s.id,
            name: s.name,
            description: s.description,
        })
        .collect();

    Ok(GetHttpTransportServicesResponse { transport_services }.into())
}

async fn create_http_endpoint(
    Extension(req_ctx): Extension<RequestContext>,
    Json(req): Json<CreateHttpEndpointRequest>,
) -> MaybeSuccessResponse<CreateHttpEndpointResponse> {
    req.validate()?;

    let context = &req_ctx.global;
    let user = &req_ctx.user_session()?.user;

    let endpoint = context
        .transaction(async |conn| {
            HttpService::new(context)
                .create_endpoint(
                    conn,
                    user,
                    req.transport_service_id,
                    &req.code,
                    &req.name,
                    req.description.as_deref(),
                )
                .await
        })
        .await?;

    Ok(CreateHttpEndpointResponse { id: endpoint.id }.into())
}

async fn delete_endpoint(
    Extension(req_ctx): Extension<RequestContext>,
    Path(endpoint_id): Path<Id>,
) -> MaybeSuccessResponse<()> {
    let context = &req_ctx.global;

    let user = &req_ctx.user_session()?.user;
    let _ = get_endpoint_and_assure_user(context, endpoint_id, user).await?;

    context
        .transaction(async |conn| {
            context
                .endpoint_service()
                .delete_by_id(conn, endpoint_id)
                .await
        })
        .await?;

    Ok(().into())
}

async fn get_endpoint_secrets(
    Extension(req_ctx): Extension<RequestContext>,
    Path(endpoint_id): Path<Id>,
) -> MaybeSuccessResponse<GetEndpointSecretsResponse> {
    let context = &req_ctx.global;

    let user = &req_ctx.user_session()?.user;
    let _ = get_endpoint_and_assure_user(context, endpoint_id, user).await?;

    let endpoint_secrets = context
        .endpoint_service()
        .find_aggregated_enabled_secrets_by_id(context.db(), endpoint_id)
        .await?;

    let mut secrets = Vec::with_capacity(endpoint_secrets.len());
    for endpoint_secret in endpoint_secrets {
        let revertable = context
            .endpoint_service()
            .count_endpoint_secrets_group_by_code(context.db(), endpoint_id, &endpoint_secret.code)
            .await?
            > 1;
        secrets.push(EndpointSecretInfo {
            id: endpoint_secret.id,
            code: endpoint_secret.code,
            endpoint_id: endpoint_secret.endpoint_id,
            secret_type: endpoint_secret.secret_type,
            created_at: endpoint_secret.created_at,
            updated_at: endpoint_secret.updated_at,
            expired_at: endpoint_secret.expired_at,
            revertable,
        });
    }

    Ok(GetEndpointSecretsResponse { secrets }.into())
}

async fn create_endpoint_secret(
    Extension(req_ctx): Extension<RequestContext>,
    Path(endpoint_id): Path<Id>,
    TypedMultipart(req): TypedMultipart<CreateEndpointSecretRequest>,
) -> MaybeSuccessResponse<CreateEndpointSecretResponse> {
    req.validate()?;

    let context = &req_ctx.global;

    let user = &req_ctx.user_session()?.user;
    let endpoint = get_endpoint_and_assure_user(context, endpoint_id, user).await?;

    let secret = if let Some(secret) = req.secret {
        Some(secret)
    } else if let Some(base64_secret) = req.base64_secret {
        BASE64_STANDARD
            .decode(base64_secret)
            .map(Bytes::from)
            .map(Some)
            .map_err(|_| error::invalid_request("Invalid base64 secret"))?
    } else {
        None
    };

    let base64_secret = match endpoint.transport_service_type {
        TransportServiceType::Telegram => {
            // Currently there is no endpoint secret for telegram
            None
        }
        TransportServiceType::Http => {
            let is_encrypt_secret = req.secret_type == HttpService::E_SECRET_TYPE_ENCRYPT_SECRET;
            let (_, secret) = context
                .transaction(async |conn| {
                    HttpService::new(context)
                        .create_endpoint_secret(
                            conn,
                            &endpoint,
                            Cow::Owned(req.code),
                            Cow::Owned(req.secret_type),
                            secret,
                        )
                        .await
                })
                .await?;
            if is_encrypt_secret {
                // encode secret
                Some(BASE64_STANDARD.encode(secret))
            } else {
                None
            }
        }
    };

    Ok(CreateEndpointSecretResponse { base64_secret }.into())
}

async fn update_endpoint_secret(
    Extension(req_ctx): Extension<RequestContext>,
    Path(endpoint_id): Path<Id>,
    Query(query): Query<EndpointSecretQuery>,
    TypedMultipart(req): TypedMultipart<UpdateEndpointSecretRequest>,
) -> MaybeSuccessResponse<CreateEndpointSecretResponse> {
    query.validate()?;

    let context = &req_ctx.global;

    let user = &req_ctx.user_session()?.user;
    let endpoint = get_endpoint_and_assure_user(context, endpoint_id, user).await?;

    let secret = if let Some(secret) = req.secret {
        Some(secret)
    } else if let Some(base64_secret) = req.base64_secret {
        BASE64_STANDARD
            .decode(base64_secret)
            .map(Bytes::from)
            .map(Some)
            .map_err(|_| error::invalid_request("Invalid base64 secret"))?
    } else {
        None
    };

    let base64_secret = match endpoint.transport_service_type {
        TransportServiceType::Telegram => {
            // Currently there is no endpoint secret for telegram
            None
        }
        TransportServiceType::Http => {
            let is_encrypt_secret = query.secret_type == HttpService::E_SECRET_TYPE_ENCRYPT_SECRET;
            let (_, secret) = context
                .transaction(async |conn| {
                    HttpService::new(context)
                        .update_endpoint_secret(
                            conn,
                            &endpoint,
                            Cow::Owned(query.code),
                            Cow::Owned(query.secret_type),
                            secret,
                        )
                        .await
                })
                .await?;
            if is_encrypt_secret {
                // encode secret
                Some(BASE64_STANDARD.encode(secret))
            } else {
                None
            }
        }
    };

    Ok(CreateEndpointSecretResponse { base64_secret }.into())
}

async fn delete_endpoint_secret(
    Extension(req_ctx): Extension<RequestContext>,
    Path(endpoint_id): Path<Id>,
    Query(query): Query<EndpointSecretQuery>,
) -> MaybeSuccessResponse<()> {
    query.validate()?;

    let context = &req_ctx.global;

    let user = &req_ctx.user_session()?.user;
    let endpoint = get_endpoint_and_assure_user(context, endpoint_id, user).await?;
    match endpoint.transport_service_type {
        TransportServiceType::Telegram => {}
        TransportServiceType::Http => {
            let options = HttpService::extract_endpoint_options(&endpoint.options)?;
            if options
                .esc_in_push()
                .filter(|esc| *esc == query.code)
                .is_some()
            {
                return Err(error::conflict("Endpoint secret is in use"));
            }
        }
    }

    context
        .transaction(async |conn| {
            context
                .endpoint_service()
                .delete_endpoint_secret_by_code_and_secret_type(
                    conn,
                    endpoint_id,
                    &query.code,
                    &query.secret_type,
                )
                .await
        })
        .await?;

    Ok(().into())
}

fn mask_base64_secret(secret: &str) -> String {
    let num_equation = if secret.ends_with("==") {
        2
    } else if secret.ends_with("=") {
        1
    } else {
        0
    };
    let valid_len = secret.len() - num_equation;

    // mask the secret according to the valid length
    if valid_len <= 8 {
        // show first 1 and last 1 character
        let first = &secret[0..1];
        let last = &secret[valid_len - 1..valid_len];
        format!(
            "{}{}{}{}",
            first,
            "*".repeat(valid_len - 2),
            last,
            "=".repeat(num_equation)
        )
    } else if valid_len <= 16 {
        // show first 2 and last 2 character
        let first = &secret[0..2];
        let last = &secret[valid_len - 2..valid_len];
        format!(
            "{}{}{}{}",
            first,
            "*".repeat(valid_len - 4),
            last,
            "=".repeat(num_equation)
        )
    } else {
        // show first 4 and last 4 character
        let first = &secret[0..4];
        let last = &secret[valid_len - 4..valid_len];
        format!(
            "{}{}{}{}",
            first,
            "*".repeat(valid_len - 8),
            last,
            "=".repeat(num_equation)
        )
    }
}

async fn get_masked_endpoint_secret(
    Extension(req_ctx): Extension<RequestContext>,
    Path((endpoint_id, endpoint_secret_id)): Path<(Id, Id)>,
) -> MaybeSuccessResponse<GetMaskedEndpointSecretResponse> {
    let context = &req_ctx.global;

    let user = &req_ctx.user_session()?.user;
    let _ = get_endpoint_and_assure_user(context, endpoint_id, user).await?;

    let endpoint_secret = context
        .endpoint_service()
        .find_endpoint_secret_by_id(context.db(), endpoint_secret_id)
        .await?;

    let secret = endpoint_secret.and_then(|(e, s)| {
        if e.endpoint_id != endpoint_id {
            tracing::warn!("Get the masked secret of EndpointSecret[{endpoint_secret_id}], but it belongs to Endpoint[{}] instead of [{endpoint_id}]", e.endpoint_id);
            None
        } else {
            if s.is_none() {
                tracing::warn!("Get the masked secret of EndpointSecret[{endpoint_secret_id}], but there is no related Secret record");
            }
            s
        }
    });

    let masked_secret = if let Some(secret) = secret {
        let mut secret = context
            .secret_service()
            .decrypt_secret(context.db(), &secret)
            .await?;
        let encoded_secret = BASE64_STANDARD.encode(&secret);
        secret.fill(0);
        Some(mask_base64_secret(&encoded_secret))
    } else {
        None
    };

    Ok(GetMaskedEndpointSecretResponse { masked_secret }.into())
}

async fn get_endpoint_subscriptions(
    Extension(req_ctx): Extension<RequestContext>,
    Path(endpoint_id): Path<Id>,
    Query(pagination): Query<PaginationQuery>,
) -> MaybeSuccessResponse<GetEndpointSubscriptionsResponse> {
    pagination.validate()?;
    let context = &req_ctx.global;

    let user = &req_ctx.user_session()?.user;
    let _ = get_endpoint_and_assure_user(context, endpoint_id, user).await?;

    let endpoint_subscriptions = context
        .subscription_service()
        .list_pagination_subscription(
            context.db(),
            Cond::all().add(SubscriptionColumn::EndpointId.eq(endpoint_id)),
            vec![(
                SubscriptionColumn::CreatedAt.into_simple_expr(),
                Order::Desc,
            )],
            (pagination.page - 1) * pagination.size,
            pagination.size,
        )
        .await?;

    let topic_ids: Vec<_> = endpoint_subscriptions
        .iter()
        .map(|s| s.0.topic_id)
        .collect();
    let topics: HashMap<_, _> = context
        .topic_service()
        .find_topics_with_latest_message_id(context.db(), topic_ids)
        .await?
        .into_iter()
        .map(|(t, m)| (t.id, (t, m)))
        .collect();

    let mut subscriptions = Vec::with_capacity(endpoint_subscriptions.len());

    for (subscription, offset) in endpoint_subscriptions {
        let Some((topic, latest_message_id)) = topics.get(&subscription.topic_id) else {
            return Err(error::internal_server_error(format!(
                "Topic[{}] not exist",
                subscription.topic_id
            )));
        };
        let Some(offset) = offset else {
            return Err(error::internal_server_error(format!(
                "Subscription[{}] without offset record",
                subscription.id
            )));
        };
        subscriptions.push(EndpointSubscriptionInfo {
            id: subscription.id,
            topic_id: subscription.topic_id,
            topic_name: topic.name.clone(),
            topic_code: topic.code.clone(),
            committed_offset: offset.committed_offset(),
            latest_offset: *latest_message_id,
            created_at: subscription.created_at,
        });
    }

    Ok(GetEndpointSubscriptionsResponse { subscriptions }.into())
}

async fn delete_endpoint_subscription(
    Extension(req_ctx): Extension<RequestContext>,
    Path((endpoint_id, subscription_id)): Path<(Id, Id)>,
) -> MaybeSuccessResponse<()> {
    let context = &req_ctx.global;

    let user = &req_ctx.user_session()?.user;
    let endpoint = get_endpoint_and_assure_user(context, endpoint_id, user).await?;
    let Some(subscription) = context
        .subscription_service()
        .find_by_id(context.db(), subscription_id)
        .await?
    else {
        return Ok(().into());
    };
    if subscription.endpoint_id != endpoint_id {
        return Err(error::invalid_request("Invalid subscription id"));
    }

    context
        .transaction(async |conn| {
            context
                .topic_service()
                .unsubscribe(conn, subscription.topic_id, &endpoint)
                .await
        })
        .await?;

    Ok(().into())
}

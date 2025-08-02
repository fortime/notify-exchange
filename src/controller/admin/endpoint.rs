use std::{collections::HashMap, collections::HashSet, sync::Arc};

use axum::{
    Extension, Router,
    extract::{Path, Query},
    middleware, routing,
};
use sea_orm::{
    ColumnTrait as _, IntoSimpleExpr as _, Order, prelude::DateTimeUtc, sea_query::Cond,
};
use serde::Serialize;
use validator::Validate;

use crate::{
    auth,
    context::RequestContext,
    controller::{MaybeSuccessResponse, PaginationQuery},
    db::{
        custom_type::Id,
        entity::{Role, SubscriptionColumn},
    },
    error,
};

pub fn route<S>(router: Router<S>) -> Router<S>
where
    S: 'static + Clone + Sync + Send,
{
    let sub_router = Router::<S>::new()
        .route("/{:endpoint_id}", routing::get(get_endpoint_info))
        .route(
            "/{:endpoint_id}/subscription",
            routing::get(get_endpoint_subscriptions),
        )
        .route_layer(middleware::from_fn_with_state(
            Arc::new(HashSet::from([Role::ADMIN])),
            auth::check_user_role_middleware,
        ))
        .route_layer(middleware::from_fn(auth::web_session_auth_middleware));

    router.nest("/v1/admin/endpoint", sub_router)
}

#[derive(Serialize)]
struct AdminGetEndpointInfoResponse {
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
struct AdminEndpointSubscriptionInfo {
    id: Id,
    topic_id: Id,
    topic_name: String,
    topic_code: String,
    committed_offset: i64,
    latest_offset: i64,
    created_at: DateTimeUtc,
}

#[derive(Serialize)]
struct AdminGetEndpointSubscriptionsResponse {
    subscriptions: Vec<AdminEndpointSubscriptionInfo>,
}

async fn get_endpoint_info(
    Extension(req_ctx): Extension<RequestContext>,
    Path(endpoint_id): Path<Id>,
) -> MaybeSuccessResponse<AdminGetEndpointInfoResponse> {
    let context = &req_ctx.global;

    let endpoint = context
        .endpoint_service()
        .find_by_id(context.db(), endpoint_id)
        .await?
        .ok_or_else(|| error::not_found("Endpoint not found"))?;

    let transport_service = context
        .transport_service_service()
        .find_by_id(context.db(), endpoint.transport_service_id)
        .await?;

    let owner = context
        .user_service()
        .find_user_by_id(context.db(), endpoint.user_id)
        .await?;

    Ok(AdminGetEndpointInfoResponse {
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
        owner_name: owner.username,
        transport_service_name: transport_service
            .as_ref()
            .map(|ts| ts.name.clone())
            .unwrap_or_else(|| "DELETED".to_string()),
    }
    .into())
}

async fn get_endpoint_subscriptions(
    Extension(req_ctx): Extension<RequestContext>,
    Path(endpoint_id): Path<Id>,
    Query(pagination): Query<PaginationQuery>,
) -> MaybeSuccessResponse<AdminGetEndpointSubscriptionsResponse> {
    pagination.validate()?;
    let context = &req_ctx.global;

    // Verify endpoint exists
    let _endpoint = context
        .endpoint_service()
        .find_by_id(context.db(), endpoint_id)
        .await?
        .ok_or_else(|| error::not_found("Endpoint not found"))?;

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
        subscriptions.push(AdminEndpointSubscriptionInfo {
            id: subscription.id,
            topic_id: subscription.topic_id,
            topic_name: topic.name.clone(),
            topic_code: topic.code.clone(),
            committed_offset: offset.committed_offset(),
            latest_offset: *latest_message_id,
            created_at: subscription.created_at,
        });
    }

    Ok(AdminGetEndpointSubscriptionsResponse { subscriptions }.into())
}

use std::{collections::HashSet, sync::Arc};

use axum::{
    Extension, Json, Router,
    extract::{Path, Query},
    middleware, routing,
};
use sea_orm::{
    ColumnTrait as _, IntoSimpleExpr as _, Order, prelude::DateTimeUtc, sea_query::Cond,
};
use serde::Serialize;
use validator::Validate;

use crate::{
    auth::{self},
    context::RequestContext,
    controller::{MaybeSuccessResponse, PaginationQuery},
    db::{
        custom_type::{Id, TopicPermissionType},
        entity::{Role, SubscriptionColumn, TopicColumn},
    },
    error,
    model::req::CreateTopicRequest,
};

pub fn route<S>(router: Router<S>) -> Router<S>
where
    S: 'static + Clone + Sync + Send,
{
    let sub_router = Router::<S>::new()
        .route("/", routing::get(get_topics))
        .route("/", routing::post(create_topic))
        .route("/{:topic_id}", routing::delete(delete_topic))
        .route(
            "/{:topic_id}/subscription",
            routing::get(get_topic_subscriptions),
        )
        .route(
            "/{:topic_id}/user",
            routing::get(get_topic_user_permissions),
        )
        .route(
            "/{:topic_id}/user/{:user_id}/permission",
            routing::put(set_topic_user_permissions),
        )
        .route_layer(middleware::from_fn_with_state(
            Arc::new(HashSet::from([Role::ADMIN])),
            auth::check_user_role_middleware,
        ))
        .route_layer(middleware::from_fn(auth::web_session_auth_middleware));

    router.nest("/v1/admin/topic", sub_router)
}

#[derive(Serialize)]
struct TopicInfo {
    id: Id,
    code: String,
    name: String,
    description: Option<String>,
    created_at: DateTimeUtc,
    owner_id: Id,
    owner_name: String,
    latest_message_offset: i64,
}

#[derive(Serialize)]
struct GetTopicsResponse {
    topics: Vec<TopicInfo>,
}

#[derive(Serialize)]
struct TopicSubscriptionInfo {
    id: Id,
    endpoint_id: Id,
    endpoint_name: String,
    endpoint_code: String,
    owner_id: Id,
    owner_name: String,
    committed_offset: i64,
    created_at: DateTimeUtc,
}

#[derive(Serialize)]
struct GetTopicSubscriptionsResponse {
    subscriptions: Vec<TopicSubscriptionInfo>,
}

#[derive(Serialize)]
pub struct TopicUserPermissionInfo {
    pub user_id: Id,
    pub username: String,
    pub email: String,
    pub permissions: Vec<TopicPermissionType>,
}

#[derive(Serialize)]
pub struct GetTopicUserPermissionsResponse {
    pub users: Vec<TopicUserPermissionInfo>,
}

#[derive(serde::Deserialize)]
pub struct SetTopicUserPermissionsRequest {
    pub assign_new_user: bool,
    pub permissions: Vec<TopicPermissionType>,
}

async fn get_topics(
    Extension(req_ctx): Extension<RequestContext>,
    Query(pagination): Query<PaginationQuery>,
) -> MaybeSuccessResponse<GetTopicsResponse> {
    pagination.validate()?;
    let context = &req_ctx.global;

    let topics = context
        .topic_service()
        .list_pagination_topic(
            context.db(),
            Cond::all(),
            vec![(TopicColumn::CreatedAt.into_simple_expr(), Order::Desc)],
            pagination.offset(),
            pagination.size,
        )
        .await?;

    let topic_ids: HashSet<Id> = topics.iter().map(|t| t.id).collect();
    let offsets = context
        .topic_service()
        .find_latest_message_ids(topic_ids.clone())
        .await?;
    let user_ids: HashSet<Id> = topics.iter().map(|t| t.user_id).collect();
    let usernames = context
        .user_service()
        .find_usernames(context.db(), user_ids)
        .await?;

    let mut topic_infos = Vec::with_capacity(topics.len());

    for topic in topics {
        topic_infos.push(TopicInfo {
            id: topic.id,
            code: topic.code,
            name: topic.name,
            description: topic.description,
            created_at: topic.created_at,
            owner_id: topic.user_id,
            owner_name: usernames
                .get(&topic.user_id)
                .cloned()
                .unwrap_or_else(|| "DELETED".to_string()),
            latest_message_offset: offsets.get(&topic.id).cloned().unwrap_or(0),
        });
    }

    Ok(GetTopicsResponse {
        topics: topic_infos,
    }
    .into())
}

async fn create_topic(
    Extension(req_ctx): Extension<RequestContext>,
    Json(req): Json<CreateTopicRequest<'_>>,
) -> MaybeSuccessResponse<Id> {
    let context = &req_ctx.global;
    let user = &req_ctx.user_session()?.user;

    let topic = context
        .transaction(async |conn| {
            context
                .topic_service()
                .create_topic(conn, user.id, req)
                .await
        })
        .await?;

    Ok(topic.id.into())
}

async fn delete_topic(
    Extension(req_ctx): Extension<RequestContext>,
    Path(topic_id): Path<Id>,
) -> MaybeSuccessResponse<()> {
    let context = &req_ctx.global;
    let user = &req_ctx.user_session()?.user;

    context
        .transaction(async |conn| {
            context
                .topic_service()
                .delete_topic(conn, user.id, topic_id)
                .await
        })
        .await?;

    Ok(().into())
}

async fn get_topic_subscriptions(
    Extension(req_ctx): Extension<RequestContext>,
    Path(topic_id): Path<Id>,
    Query(pagination): Query<PaginationQuery>,
) -> MaybeSuccessResponse<GetTopicSubscriptionsResponse> {
    pagination.validate()?;
    let context = &req_ctx.global;

    let topic_subscriptions = context
        .subscription_service()
        .list_pagination_subscription(
            context.db(),
            Cond::all().add(SubscriptionColumn::TopicId.eq(topic_id)),
            vec![(
                SubscriptionColumn::CreatedAt.into_simple_expr(),
                Order::Desc,
            )],
            pagination.offset(),
            pagination.size,
        )
        .await?;

    let endpoint_ids: HashSet<Id> = topic_subscriptions
        .iter()
        .map(|s| s.0.endpoint_id)
        .collect();
    let endpoints = context
        .endpoint_service()
        .find_by_ids(context.db(), endpoint_ids)
        .await?;
    let user_ids: HashSet<Id> = endpoints.iter().map(|e| e.1.user_id).collect();
    let users = context
        .user_service()
        .find_usernames(context.db(), user_ids)
        .await?;

    let mut subscriptions = Vec::with_capacity(topic_subscriptions.len());

    for (subscription, offset) in topic_subscriptions {
        let Some(endpoint) = endpoints.get(&subscription.endpoint_id) else {
            return Err(error::internal_server_error(format!(
                "Endpoint[{}] not exist",
                subscription.endpoint_id
            )));
        };
        let Some(offset) = offset else {
            return Err(error::internal_server_error(format!(
                "Subscription[{}] without offset record",
                subscription.id
            )));
        };
        subscriptions.push(TopicSubscriptionInfo {
            id: subscription.id,
            endpoint_id: endpoint.id,
            endpoint_name: endpoint.name.clone(),
            endpoint_code: endpoint.code.clone(),
            owner_id: endpoint.user_id,
            owner_name: users
                .get(&endpoint.user_id)
                .cloned()
                .unwrap_or_else(|| "DELETED".to_string()),
            committed_offset: offset.committed_offset(),
            created_at: subscription.created_at,
        });
    }

    Ok(GetTopicSubscriptionsResponse { subscriptions }.into())
}

async fn get_topic_user_permissions(
    Extension(req_ctx): Extension<RequestContext>,
    Path(topic_id): Path<Id>,
    Query(pagination): Query<PaginationQuery>,
) -> MaybeSuccessResponse<GetTopicUserPermissionsResponse> {
    pagination.validate()?;

    let context = &req_ctx.global;

    let users = context
        .topic_service()
        .list_pagination_topic_user_permissions(
            context.db(),
            topic_id,
            pagination.offset(),
            pagination.size,
        )
        .await?
        .into_iter()
        .map(|(user, permissions)| TopicUserPermissionInfo {
            user_id: user.id,
            username: user.username,
            email: user.email,
            permissions,
        })
        .collect();

    Ok(GetTopicUserPermissionsResponse { users }.into())
}

async fn set_topic_user_permissions(
    Extension(req_ctx): Extension<RequestContext>,
    Path((topic_id, user_id)): Path<(Id, Id)>,
    Json(req): Json<SetTopicUserPermissionsRequest>,
) -> MaybeSuccessResponse<()> {
    let context = &req_ctx.global;
    let operator = &req_ctx.user_session()?.user;

    context
        .transaction(async |conn| {
            context
                .topic_service()
                .set_user_topic_permissions(
                    conn,
                    operator.id,
                    topic_id,
                    user_id,
                    req.permissions,
                    req.assign_new_user,
                )
                .await
        })
        .await?;

    Ok(().into())
}

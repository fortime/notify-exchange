use std::collections::HashSet;

use axum::{
    Extension, Json, Router,
    extract::{Path, Query},
    middleware, routing,
};
use base64::{Engine as _, prelude::BASE64_STANDARD};
use sea_orm::{
    ColumnTrait as _, IntoSimpleExpr as _, Order, prelude::DateTimeUtc, sea_query::Cond,
};
use serde::{Deserialize, Serialize};
use validator::Validate;

use crate::{
    auth,
    context::RequestContext,
    controller::{
        self, MaybeSuccessResponse, PaginationQuery,
        admin::topic::{
            GetTopicUserPermissionsResponse, SetTopicUserPermissionsRequest,
            TopicUserPermissionInfo,
        },
    },
    db::{
        custom_type::{Id, TopicPermissionType},
        entity::{SubscriptionColumn, TopicColumn},
    },
    error,
};

pub fn route<S>(router: Router<S>) -> Router<S>
where
    S: 'static + Clone + Sync + Send,
{
    let sub_router = Router::<S>::new()
        .route("/", routing::get(get_user_topics))
        .route("/{:topic_id}", routing::get(get_user_topic))
        .route(
            "/{:topic_id}/subscription",
            routing::get(get_user_topic_subscriptions).post(subscribe),
        )
        .route("/{:topic_id}/message", routing::get(get_topic_messages))
        .route(
            "/{:topic_id}/user",
            routing::get(get_topic_user_permissions),
        )
        .route(
            "/{:topic_id}/user/{:user_id}/permission",
            routing::put(set_topic_user_permissions),
        )
        .route_layer(middleware::from_fn(auth::web_session_auth_middleware));
    router.nest("/v1/topic", sub_router)
}

#[derive(Deserialize)]
struct GetUserTopicsQuery {
    subscribed: Option<bool>,
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
    subscribed: bool,
    latest_message_offset: i64,
}

#[derive(Serialize)]
struct GetUserTopicsResponse {
    topics: Vec<(TopicInfo, Vec<TopicPermissionType>)>,
}

#[derive(Serialize)]
struct GetUserTopicResponse {
    topic: TopicInfo,
}

#[derive(Serialize)]
struct TopicSubscriptionInfo {
    id: Id,
    endpoint_id: Id,
    endpoint_name: String,
    endpoint_code: String,
    committed_offset: i64,
    created_at: DateTimeUtc,
}

#[derive(Serialize)]
struct GetUserTopicSubscriptionsResponse {
    subscriptions: Vec<TopicSubscriptionInfo>,
}

#[derive(Deserialize)]
struct SubscribeRequest {
    endpoint_id: Id,
}

#[derive(Deserialize)]
struct GetTopicMessagesQuery {
    max_offset: Option<i64>,
}

#[derive(Serialize)]
struct MessageInfo {
    offset: i64,
    content: String,
    is_binary: bool,
}

#[derive(Serialize)]
struct GetTopicMessageResponse {
    messages: Vec<MessageInfo>,
    next_max_offset: Option<i64>,
}

async fn get_user_topics(
    Extension(req_ctx): Extension<RequestContext>,
    Query(query): Query<GetUserTopicsQuery>,
    Query(pagination): Query<PaginationQuery>,
) -> MaybeSuccessResponse<GetUserTopicsResponse> {
    pagination.validate()?;
    let context = &req_ctx.global;
    let topic_service = context.topic_service();

    let user = &req_ctx.user_session()?.user;

    let topics = topic_service
        .list_pagination_authorized_topic(
            context.db(),
            user.id,
            query.subscribed,
            Cond::all(),
            vec![(TopicColumn::CreatedAt.into_simple_expr(), Order::Desc)],
            pagination.offset(),
            pagination.size,
        )
        .await?;

    let topic_ids: HashSet<Id> = topics.iter().map(|t| t.id).collect();
    let offsets = topic_service
        .find_latest_message_ids(topic_ids.clone())
        .await?;
    let user_ids: HashSet<Id> = topics.iter().map(|t| t.user_id).collect();
    let usernames = context
        .user_service()
        .find_usernames(context.db(), user_ids)
        .await?;

    let mut topic_infos = Vec::with_capacity(topics.len());
    let subscribed_topic_ids = context
        .subscription_service()
        .filter_subscribed_topics(context.db(), user.id, topic_ids)
        .await?;

    for topic in topics {
        let user_permissions = topic_service
            .find_user_topic_permissions(context.db(), topic.id, user.id)
            .await?
            .iter()
            .map(|up| up.permission_type)
            .collect();
        topic_infos.push((
            TopicInfo {
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
                subscribed: subscribed_topic_ids.contains(&topic.id),
                latest_message_offset: offsets.get(&topic.id).cloned().unwrap_or(0),
            },
            user_permissions,
        ));
    }

    Ok(GetUserTopicsResponse {
        topics: topic_infos,
    }
    .into())
}

async fn get_user_topic(
    Extension(req_ctx): Extension<RequestContext>,
    Path(topic_id): Path<Id>,
) -> MaybeSuccessResponse<GetUserTopicResponse> {
    let context = &req_ctx.global;

    let user = &req_ctx.user_session()?.user;

    let topic = context
        .topic_service()
        .find_by_id(context.db(), topic_id)
        .await?;

    let owner_name = if topic.user_id == user.id {
        user.username.clone()
    } else {
        context
            .user_service()
            .find_user_by_id(context.db(), topic.user_id)
            .await?
            .username
    };

    let subscribed = !context
        .subscription_service()
        .filter_subscribed_topics(context.db(), user.id, [topic_id])
        .await?
        .is_empty();

    let latest_message_offset = context
        .topic_service()
        .find_latest_message_id_by_topic(topic_id)
        .await?
        .unwrap_or(0);

    Ok(GetUserTopicResponse {
        topic: TopicInfo {
            id: topic.id,
            code: topic.code,
            name: topic.name,
            description: topic.description,
            created_at: topic.created_at,
            owner_id: topic.user_id,
            owner_name,
            subscribed,
            latest_message_offset,
        },
    }
    .into())
}

async fn get_user_topic_subscriptions(
    Extension(req_ctx): Extension<RequestContext>,
    Path(topic_id): Path<Id>,
    Query(pagination): Query<PaginationQuery>,
) -> MaybeSuccessResponse<GetUserTopicSubscriptionsResponse> {
    pagination.validate()?;
    let context = &req_ctx.global;

    let user = &req_ctx.user_session()?.user;

    let topic_subscriptions = context
        .subscription_service()
        .list_pagination_subscription(
            context.db(),
            Cond::all()
                .add(SubscriptionColumn::UserId.eq(user.id))
                .add(SubscriptionColumn::TopicId.eq(topic_id)),
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
            committed_offset: offset.committed_offset(),
            created_at: subscription.created_at,
        });
    }

    Ok(GetUserTopicSubscriptionsResponse { subscriptions }.into())
}

async fn subscribe(
    Extension(req_ctx): Extension<RequestContext>,
    Path(topic_id): Path<Id>,
    Json(req): Json<SubscribeRequest>,
) -> MaybeSuccessResponse<()> {
    let context = &req_ctx.global;

    let user = &req_ctx.user_session()?.user;

    let endpoint = context
        .endpoint_service()
        .find_by_id(context.db(), req.endpoint_id)
        .await?
        .ok_or_else(|| error::not_found("Endpoint not found"))?;
    controller::assure_user(user.id, endpoint.user_id)?;

    context
        .transaction(async |conn| {
            let subscription = context
                .subscription_service()
                .find_subscription(conn, topic_id, endpoint.id)
                .await?;
            if let Some(subscription) = subscription {
                return Ok(subscription);
            }
            context
                .topic_service()
                .subscribe(conn, topic_id, &endpoint, user.id)
                .await
        })
        .await?;

    Ok(().into())
}

async fn get_topic_messages(
    Extension(req_ctx): Extension<RequestContext>,
    Path(topic_id): Path<Id>,
    Query(query): Query<GetTopicMessagesQuery>,
) -> MaybeSuccessResponse<GetTopicMessageResponse> {
    let context = &req_ctx.global;

    let user = &req_ctx.user_session()?.user;

    let (encrypted_messages, next_max_offset) = context
        .topic_service()
        .list_topic_message(context.db(), topic_id, user.id, query.max_offset, 5)
        .await?;

    let mut messages = Vec::with_capacity(encrypted_messages.len());

    for message in encrypted_messages {
        let content = context
            .secret_service()
            .decrypt_secret(context.db(), &message)
            .await?;
        let (content, is_binary) = match String::from_utf8(content.clone()) {
            Ok(c) => (c, false),
            Err(_) => (BASE64_STANDARD.encode(content), true),
        };
        messages.push(MessageInfo {
            offset: message.id,
            content,
            is_binary,
        })
    }

    Ok(GetTopicMessageResponse {
        messages,
        next_max_offset,
    }
    .into())
}

async fn get_topic_user_permissions(
    Extension(req_ctx): Extension<RequestContext>,
    Path(topic_id): Path<Id>,
    Query(pagination): Query<PaginationQuery>,
) -> MaybeSuccessResponse<GetTopicUserPermissionsResponse> {
    pagination.validate()?;

    let context = &req_ctx.global;

    let user = &req_ctx.user_session()?.user;
    if !context
        .topic_service()
        .has_manage_permissions(context.db(), topic_id, user.id)
        .await?
    {
        return Err(error::forbidden("No permission"));
    }

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

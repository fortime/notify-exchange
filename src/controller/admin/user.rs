use std::{borrow::Cow, collections::HashSet, sync::Arc};

use axum::{
    Extension, Json, Router,
    extract::{Path, Query},
    middleware, routing,
};
use sea_orm::{
    ColumnTrait as _, IntoSimpleExpr as _, Order, prelude::DateTimeUtc, sea_query::Cond,
};
use serde::{Deserialize, Serialize};
use validator::Validate;

use crate::{
    auth::{self},
    context::RequestContext,
    controller::{MaybeSuccessResponse, PaginationQuery},
    db::{
        custom_type::Id,
        entity::{Role, UserColumn},
    },
};

pub fn route<S>(router: Router<S>) -> Router<S>
where
    S: 'static + Clone + Sync + Send,
{
    let sub_router = Router::<S>::new()
        .route("/", routing::get(get_users))
        .route("/{:user_id}/enabled", routing::put(set_user_enabled))
        .route_layer(middleware::from_fn_with_state(
            Arc::new(HashSet::from([Role::ADMIN])),
            auth::check_user_role_middleware,
        ))
        .route_layer(middleware::from_fn(auth::web_session_auth_middleware));

    router.nest("/v1/admin/user", sub_router)
}

#[derive(Serialize)]
struct UserInfo {
    id: Id,
    username: String,
    email: String,
    enabled: bool,
    created_at: DateTimeUtc,
}

#[derive(Serialize)]
struct GetUsersResponse {
    users: Vec<UserInfo>,
}

#[derive(Deserialize)]
struct GetUsersQuery {
    pub search: Option<String>,
}

async fn get_users(
    Extension(req_ctx): Extension<RequestContext>,
    Query(query): Query<GetUsersQuery>,
    Query(pagination): Query<PaginationQuery>,
) -> MaybeSuccessResponse<GetUsersResponse> {
    pagination.validate()?;
    let context = &req_ctx.global;

    let mut cond = Cond::all();
    if let Some(search) = query.search {
        cond = cond.add(
            Cond::any()
                .add(UserColumn::Username.like(format!("{search}%")))
                .add(UserColumn::Email.like(format!("{search}%"))),
        );
    }

    let users = context
        .user_service()
        .list_pagination_user(
            context.db(),
            cond,
            vec![(UserColumn::CreatedAt.into_simple_expr(), Order::Desc)],
            pagination.offset(),
            pagination.size,
        )
        .await?;

    let users = users
        .into_iter()
        .map(|u| UserInfo {
            id: u.id,
            username: u.username,
            email: u.email,
            enabled: u.enabled,
            created_at: u.created_at,
        })
        .collect();

    Ok(GetUsersResponse { users }.into())
}

async fn set_user_enabled(
    Extension(req_ctx): Extension<RequestContext>,
    Path(user_id): Path<Id>,
    Json(enabled): Json<bool>,
) -> MaybeSuccessResponse<()> {
    let context = &req_ctx.global;

    let operator = &req_ctx.user_session()?.user;

    context
        .user_service()
        .set_user_enabled(context.db(), Cow::Borrowed(operator), user_id, enabled)
        .await?;
    Ok(().into())
}

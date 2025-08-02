use std::{collections::HashSet, sync::Arc};

use axum::{
    Extension, Router,
    extract::{Path, Query},
    middleware, routing,
};
use sea_orm::{IntoSimpleExpr as _, Order, prelude::DateTimeUtc, sea_query::Cond};
use serde::Serialize;
use validator::Validate;

use crate::{
    auth::{self},
    context::RequestContext,
    controller::{MaybeSuccessResponse, PaginationQuery},
    db::{
        custom_type::Id,
        entity::{PendingUserColumn, Role},
    },
    error,
};

pub fn route<S>(router: Router<S>) -> Router<S>
where
    S: 'static + Clone + Sync + Send,
{
    let sub_router = Router::<S>::new()
        .route("/", routing::get(get_pending_users))
        .route("/{:pending_user_id}", routing::put(confirm_pending_user))
        .route("/{:pending_user_id}", routing::delete(delete_pending_user))
        .route_layer(middleware::from_fn_with_state(
            Arc::new(HashSet::from([Role::ADMIN])),
            auth::check_user_role_middleware,
        ))
        .route_layer(middleware::from_fn(auth::web_session_auth_middleware));

    router.nest("/v1/admin/pending-user", sub_router)
}

#[derive(Serialize)]
struct PendingUserInfo {
    id: Id,
    username: String,
    email: String,
    created_at: DateTimeUtc,
    expired_at: DateTimeUtc,
}

#[derive(Serialize)]
struct GetPendingUsersResponse {
    pending_users: Vec<PendingUserInfo>,
}

async fn get_pending_users(
    Extension(req_ctx): Extension<RequestContext>,
    Query(pagination): Query<PaginationQuery>,
) -> MaybeSuccessResponse<GetPendingUsersResponse> {
    pagination.validate()?;
    let context = &req_ctx.global;
    let pending_users = context
        .user_service()
        .list_pagination_pending_user(
            context.db(),
            Cond::all(),
            vec![(PendingUserColumn::CreatedAt.into_simple_expr(), Order::Desc)],
            pagination.offset(),
            pagination.size,
        )
        .await?;

    let pending_users = pending_users
        .into_iter()
        .map(|p| PendingUserInfo {
            id: p.id,
            username: p.username,
            email: p.email,
            created_at: p.created_at,
            expired_at: p.expired_at,
        })
        .collect();

    Ok(GetPendingUsersResponse { pending_users }.into())
}

async fn confirm_pending_user(
    Extension(req_ctx): Extension<RequestContext>,
    Path(pending_user_id): Path<Id>,
) -> MaybeSuccessResponse<()> {
    let context = &req_ctx.global;
    context
        .transaction(async move |conn| {
            let (user, ext) = context
                .user_service()
                .confirm_pending_user(conn, pending_user_id)
                .await?;
            context
                .endpoint_service()
                .insert_endpoints(conn, user.id, ext.endpoints)
                .await?;
            Ok(())
        })
        .await?;

    Ok(().into())
}

async fn delete_pending_user(
    Extension(req_ctx): Extension<RequestContext>,
    Path(pending_user_id): Path<Id>,
) -> MaybeSuccessResponse<()> {
    let context = &req_ctx.global;
    let rows_affected = context
        .user_service()
        .delete_pending_user(context.db(), &[pending_user_id])
        .await?;
    if rows_affected == 0 {
        return Err(error::not_found("Pending user not found"));
    }
    Ok(().into())
}

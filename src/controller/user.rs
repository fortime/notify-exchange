use std::{borrow::Cow, time::Duration};

use axum::{Extension, Json, Router, extract::Query, http::HeaderMap, middleware, routing};
use axum_extra::extract::{CookieJar, cookie::Cookie};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::{
    auth::{self, COOKIE_SESSION_ID, SESSION_TIMEOUT},
    cache::CacheManager,
    context::RequestContext,
    controller::{MaybeSuccessResponse, Response},
    db::{custom_type::Id, entity::Role},
    error::{self, NotifyExchangeResultExt, WwwAuthenticate},
    model::cache::{LoginRequestCache, SessionCache},
};

pub fn route<S>(router: Router<S>) -> Router<S>
where
    S: 'static + Clone + Sync + Send,
{
    let sub_router = Router::<S>::new()
        .route("/me", routing::get(get_user_info))
        .route_layer(middleware::from_fn(auth::web_session_auth_middleware))
        // Because `web_session_auth_middleware` will extend the lifetime of session automatically,
        // it may return a `Set-Cookie` to update the `max-age` of the cookie. So we shouldn't use
        // `web_session_auth_middleware` in logout.
        .route(
            "/session",
            routing::post(login).get(get_login_session).delete(logout),
        )
        .route(
            "/session/telegram",
            routing::post(get_login_session_by_telegram),
        );
    router.nest("/v1/user", sub_router)
}

// `__Host-Http` is a cookie prefix for security
const COOKIE_LOGIN_CODE: &str = "__Host-Http-x-ne-login-code";

const LOGIN_REQUEST_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Deserialize, Validate)]
struct LoginRequest {
    #[validate(length(max = 128))]
    #[validate(email)]
    email: String,
}

#[derive(Serialize)]
struct LoginResponse {
    token: String,
}

#[derive(Deserialize)]
struct GetLoginSessionRequest {
    token: String,
}

#[derive(Serialize)]
struct GetLoginSessionResponse {
    /// If it is not set, it means session hasn't been created.
    csrf_token: Option<String>,
}

#[derive(Deserialize)]
struct GetLoginSessionByTelegramRequest {
    eid: Id,
    init_data: String,
}

#[derive(Serialize)]
struct GetUserInfoResponse {
    id: Id,
    username: String,
    email: String,
    is_admin: bool,
}

async fn login(
    Extension(req_ctx): Extension<RequestContext>,
    Json(req): Json<LoginRequest>,
) -> MaybeSuccessResponse<LoginResponse> {
    let context = &req_ctx.global;
    let user = context
        .user_service()
        .find_user_by_email(context.db(), &req.email)
        .await?;

    // Use v4 for security
    let code = Uuid::new_v4().to_string();
    let token = if let Some(user) = user.filter(|u| u.enabled) {
        context
            .cache_manager()
            .add_token(
                &LoginRequestCache {
                    user_id: user.id,
                    user_email: user.email,
                    code: code.clone(),
                    session_id: None,
                },
                LOGIN_REQUEST_TIMEOUT,
                10,
            )
            .await?
    } else {
        unsafe {
            // SAFETY, We will check if the token is sent from the same user during the
            // confirmation of login. And we also use a code to get the session in
            // `get_login_session`.
            CacheManager::new_token()
        }
    };

    // Only request with correct code can get the session
    let jar = CookieJar::new().add(
        Cookie::build((COOKIE_LOGIN_CODE, code))
            .path("/")
            .max_age(
                LOGIN_REQUEST_TIMEOUT
                    .try_into()
                    .whatever("Invalid time range")?,
            )
            .http_only(true)
            .secure(true),
    );

    Ok(Response::from(LoginResponse { token }).with_cookies(Some(jar)))
}

async fn get_login_session(
    Extension(req_ctx): Extension<RequestContext>,
    Query(req): Query<GetLoginSessionRequest>,
    mut jar: CookieJar,
) -> MaybeSuccessResponse<GetLoginSessionResponse> {
    let context = &req_ctx.global;
    let Some(code) = jar.get(COOKIE_LOGIN_CODE) else {
        return Err(error::invalid_request("No login code"));
    };
    let Some(login_req) = context
        .cache_manager()
        .get_token::<LoginRequestCache>(&req.token)
        .await?
    else {
        return Err(error::invalid_request("No login req"));
    };

    if login_req.code != code.value() {
        tracing::warn!(
            "Request login for toke[{}], but code[{code}] is not expected[{}]",
            req.token,
            login_req.code,
        );
        return Err(error::invalid_request("No login req"));
    }

    if login_req.session_id.is_some() {
        // Delete the cache so no other request can get the session id again.
        context
            .cache_manager()
            .delete_token::<LoginRequestCache>(&req.token)
            .await?;
    }

    let session_id = login_req.session_id;
    let csrf_token = if let Some(session_id) = session_id {
        // Check session and get csrf_token
        let Some(session) = context
            .cache_manager()
            .get_token::<SessionCache>(&session_id)
            .await?
        else {
            tracing::error!(
                "Session id is set but there is no session record, session_id: {session_id}, token: {}, code: {code}",
                req.token
            );
            return Err(error::internal_server_error("No session record found"));
        };
        // Add session id to cookies and remove login code from cookies
        jar = jar
            .add(
                Cookie::build((COOKIE_SESSION_ID, session_id))
                    .path("/")
                    .max_age(SESSION_TIMEOUT.try_into().whatever("Invalid time range")?)
                    .http_only(true)
                    .secure(true),
            )
            .remove(
                Cookie::build(COOKIE_LOGIN_CODE)
                    .path("/")
                    .http_only(true)
                    .secure(true),
            );
        Some(session.csrf_token)
    } else {
        None
    };

    Ok(Response::from(GetLoginSessionResponse { csrf_token }).with_cookies(jar.into()))
}

async fn get_login_session_by_telegram(
    Extension(req_ctx): Extension<RequestContext>,
    mut jar: CookieJar,
    Json(req): Json<GetLoginSessionByTelegramRequest>,
) -> MaybeSuccessResponse<GetLoginSessionResponse> {
    let context = &req_ctx.global;

    let user = auth::verify_telegram_login(context, &req.init_data, req.eid).await?;

    // Create Session
    let (session_id, csrf_token) = context
        .user_service()
        .login(context.db(), Cow::Owned(user))
        .await?;

    jar = jar.add(
        Cookie::build((COOKIE_SESSION_ID, session_id))
            .path("/")
            .max_age(SESSION_TIMEOUT.try_into().whatever("Invalid time range")?)
            .http_only(true)
            .secure(true),
    );

    Ok(Response::from(GetLoginSessionResponse {
        csrf_token: Some(csrf_token),
    })
    .with_cookies(jar.into()))
}

async fn logout(
    Extension(req_ctx): Extension<RequestContext>,
    mut jar: CookieJar,
) -> MaybeSuccessResponse<()> {
    let Some(session) = req_ctx.user_session else {
        return Ok(().into());
    };
    // Delete session and clear cookies
    req_ctx
        .global
        .cache_manager()
        .delete_token::<SessionCache>(&session.id)
        .await?;

    jar = jar.remove(
        Cookie::build(COOKIE_SESSION_ID)
            .path("/")
            .http_only(true)
            .secure(true),
    );
    Ok(Response::from(()).with_cookies(jar.into()))
}

async fn get_user_info(
    Extension(req_ctx): Extension<RequestContext>,
    headers: HeaderMap,
) -> MaybeSuccessResponse<GetUserInfoResponse> {
    let session = req_ctx.user_session()?;

    // Check csrf_token, if it is not the same, return 401 to let the user login again.
    if headers
        .get(auth::HEADER_CSRF_TOKEN)
        .filter(|v| v.as_bytes() == session.csrf_token.as_bytes())
        .is_none()
    {
        tracing::warn!("Invalid csrf token");
        return Err(error::unauthorized(
            "Invalid csrf token",
            vec![WwwAuthenticate::NeLogin],
        ));
    }

    let is_admin = session.roles.iter().any(Role::is_admin);
    // Query from db if we can change username and email
    Ok(GetUserInfoResponse {
        id: session.user.id,
        username: session.user.username.clone(),
        email: session.user.email.clone(),
        is_admin,
    }
    .into())
}

use std::time::Duration;

use axum::{Extension, Json, Router, response::Redirect, routing};
use either::Either;
use serde::{Deserialize, Serialize};
use validator::Validate;

use crate::{
    context::RequestContext,
    controller::{MaybeSuccessResponse, Response},
    db::{custom_type::TransportServiceType, entity::Role},
    error::{self, NotifyExchangeError, NotifyExchangeResult},
    model::req::CreateUserRequest,
    service::{transport::telegram, user::PendingUserRequestExt},
    util,
};

/// Wait for setup to finish, this is the maximum time we will wait for the setup to finish.
const SETUP_TIMEOUT: Duration = Duration::from_secs(60 * 5);

pub fn route<S>(router: Router<S>) -> Router<S>
where
    S: 'static + Clone + Sync + Send,
{
    router
        .route("/v1/admin/setup/telegram", routing::post(setup_telegram))
        .route(
            "/v1/admin/setup/transport-service-type",
            routing::get(transport_service_types),
        )
}

#[allow(unused)]
async fn redirect_setup(
    Extension(req_ctx): Extension<RequestContext>,
) -> NotifyExchangeResult<Redirect> {
    let mut url = req_ctx.global.config().base_url().clone();
    util::extend_url(&mut url, "setup")?;
    Ok(Redirect::to(url.as_ref()))
}

async fn transport_service_types() -> Response<Vec<u16>> {
    vec![TransportServiceType::Telegram as u16].into()
}

#[derive(Deserialize, Validate)]
struct SetupTelegramRequest {
    #[validate(nested)]
    pub admin: CreateUserRequest<'static>,
    #[validate(length(max = 1024))]
    pub token: String,
    #[validate(length(min = 4, max = 128))]
    pub name: String,
    #[validate(length(max = 1024))]
    pub description: Option<String>,
}

#[derive(Serialize)]
pub struct SetupTelegramResponse {
    pub token: String,
}

// #[axum::debug_handler]
async fn setup_telegram(
    Extension(req_ctx): Extension<RequestContext>,
    Json(req): Json<SetupTelegramRequest>,
) -> MaybeSuccessResponse<SetupTelegramResponse> {
    req.validate()?;
    // Check bot outside transaction
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
    let token = context
        .transaction(async |conn| {
            let transport_service_service = context.transport_service_service();
            let user_service = context.user_service();

            // Check if any transport service has been already set up
            if transport_service_service.count_enabled(conn).await? != 0 {
                return Err(error::invalid_request(
                    "The server has been already set up".to_string(),
                ));
            }

            // Only allow one setup in a time
            if user_service.any_pending_user(conn).await? {
                return Err(error::conflict(
                    "There are already a steup in progress".to_string(),
                ));
            }

            // Create a new pending user for the admin
            let mut ext = PendingUserRequestExt::default();
            ext.roles.push(Role::ADMIN.to_string());
            let pending_user = user_service
                .create_pending_user(conn, req.admin, ext, SETUP_TIMEOUT)
                .await?;

            let token = pending_user.id.to_string();
            tokio::spawn({
                let context = context.clone();
                let token = token.clone();
                async move {
                    if let Err(e) = telegram::wait_and_finish_setup(
                        context,
                        bot,
                        req.name,
                        req.description,
                        Either::Left(pending_user.id),
                        token,
                        SETUP_TIMEOUT,
                    )
                    .await
                    {
                        tracing::error!("wait_and_finish_setup error: {e:#?}");
                    }
                }
            });
            Ok(token)
        })
        .await?;
    Ok(SetupTelegramResponse { token }.into())
}

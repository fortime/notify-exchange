//! Create a simple http server to simplify the communication with notify-exchange-server. It
//! provides an api with basic access authentication.
use std::sync::Arc;

use axum::{
    Extension, Json, Router,
    extract::{Query, Request, State},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing,
};
use axum_extra::{
    TypedHeader,
    headers::{Authorization, authorization::Basic},
};
use base64::{Engine, prelude::BASE64_STANDARD};
use config::Account;
use notify_exchange::{
    controller::MaybeSuccessResponse,
    error::{self, NotifyExchangeResult, NotifyExchangeResultExt as _, WwwAuthenticate},
    util::Signals,
};
use serde::{Deserialize, Serialize};
use tokio::{net::TcpListener, signal::unix::SignalKind};
use tracing::Level;

use crate::config::Config;

mod api;
mod config;

#[derive(Deserialize)]
struct PullMessageQuery {
    topic_code: String,
    topic_user_id: Option<String>,
    limit: u8,
    poll_timeout: Option<u8>,
}

#[derive(Serialize)]
struct PullMessageResponse {
    messages: Vec<String>,
}

#[derive(Deserialize)]
struct PublishMessageRequest {
    topic_code: String,
    topic_user_id: Option<String>,
    message: String,
    base64_encoded: bool,
}

// Wait for signals and do cleaning
fn graceful_shutdown() -> NotifyExchangeResult<impl Future<Output = ()> + Send + 'static> {
    let signals = Signals::new(vec![
        SignalKind::terminate(),
        SignalKind::interrupt(),
        SignalKind::hangup(),
        SignalKind::pipe(),
        SignalKind::quit(),
    ])?;
    Ok(async move {
        tracing::info!("Waiting for signals: {:?}", signals);
        signals.await;
    })
}

async fn auth(
    State(config): State<Arc<Config>>,
    authorization: Option<TypedHeader<Authorization<Basic>>>,
    mut req: Request,
    next: Next,
) -> Response {
    let Some(TypedHeader(authorization)) = authorization else {
        return error::unauthorized(
            "Authorization is needed",
            vec![WwwAuthenticate::Basic("Notify Exchange Http Proxy")],
        )
        .into_response();
    };
    let Some(account) = config.accounts().get(authorization.username()) else {
        tracing::info!(
            "Authorization of unknown account: {}",
            authorization.username()
        );
        return error::unauthorized(
            "Invalid authorization",
            vec![WwwAuthenticate::Basic("Notify Exchange Http Proxy")],
        )
        .into_response();
    };
    if !account.authenticate(authorization.password()) {
        if tracing::enabled!(Level::DEBUG) {
            tracing::debug!(
                "Authorization of wrong password, user[{}], password[{}]",
                authorization.username(),
                authorization.password()
            );
        } else {
            tracing::info!(
                "Authorization of wrong password, user[{}]",
                authorization.username()
            );
        }
        return error::unauthorized(
            "Invalid authorization",
            vec![WwwAuthenticate::Basic("Notify Exchange Http Proxy")],
        )
        .into_response();
    }
    req.extensions_mut().insert(account.clone());
    next.run(req).await
}

async fn publish_message(
    Extension(account): Extension<Arc<Account>>,
    Json(req): Json<PublishMessageRequest>,
) -> MaybeSuccessResponse<()> {
    let topic_user_id = req.topic_user_id.as_ref().map(|i| i.parse()).transpose()?;
    let message = if req.base64_encoded {
        BASE64_STANDARD
            .decode(req.message)
            .map_err(|_| error::invalid_request("Invalid base64 encoed message"))?
    } else {
        req.message.into_bytes()
    };
    account
        .as_api_client()
        .publish_message(&req.topic_code, topic_user_id, &message)
        .await?;
    Ok(().into())
}

async fn pull_message(
    Extension(account): Extension<Arc<Account>>,
    Query(query): Query<PullMessageQuery>,
) -> MaybeSuccessResponse<PullMessageResponse> {
    let topic_user_id = query
        .topic_user_id
        .as_ref()
        .map(|i| i.parse())
        .transpose()?;
    let (sub_id, messages) = account
        .as_api_client()
        .pull_message(
            &query.topic_code,
            topic_user_id,
            query.limit,
            query.poll_timeout,
        )
        .await?;
    let mut last_offset = None;
    let mut base64_messages = vec![];
    for (offset, message) in messages {
        base64_messages.push(BASE64_STANDARD.encode(message));
        last_offset = Some(offset);
    }
    if let Some(last_offset) = last_offset {
        account
            .as_api_client()
            .commit_offset(sub_id, last_offset)
            .await?;
    }
    Ok(PullMessageResponse {
        messages: base64_messages,
    }
    .into())
}

async fn run() -> NotifyExchangeResult<()> {
    let config = Arc::new(Config::parse()?);

    let _guard = notify_exchange::init_log(config.log_timestamp())?;

    let mut app = Router::new().with_state(Arc::clone(&config));

    let listener = TcpListener::bind((config.bind_ip(), config.bind_port()))
        .await
        .whatever("Failed to listen to the address")?;

    app = app.route(
        "/message",
        routing::post(publish_message)
            .get(pull_message)
            .route_layer(middleware::from_fn_with_state(config, auth)),
    );

    axum::serve(listener, app)
        // Stop all services after receiving the stop signal
        .with_graceful_shutdown(graceful_shutdown()?)
        .await
        .whatever("Failed to start server")?;

    Ok(())
}

#[tokio::main]
async fn main() {
    #[cfg(feature = "dotenv")]
    dotenv::dotenv().ok();

    if let Err(e) = run().await {
        eprintln!("failed to notify exchange http proxy: {:#?}", e);
    }
}

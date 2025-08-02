use axum::{Extension, Router, extract::Path, http::HeaderMap, routing};
use teloxide::{
    Bot,
    dispatching::dialogue::GetChatId as _,
    prelude::Requester,
    types::{Update, UpdateKind},
};

use crate::{
    context::{Context, RequestContext},
    db::{
        custom_type::Id,
        entity::{Role, TransportService},
    },
    error::{self, NotifyExchangeError, NotifyExchangeResult},
    service::transport::telegram::{self, ChatContext, TelegramService},
};

const SECRET_TOKEN_HEADER: &str = "x-telegram-bot-api-secret-token";

pub fn route<S>(router: Router<S>) -> Router<S>
where
    S: 'static + Clone + Sync + Send,
{
    router.route(
        &format!("{}/{{transport_service_id}}", TelegramService::WEBHOOK_PATH),
        routing::post(handle),
    )
}

#[tracing::instrument(level = "info", skip(req_ctx, headers, input), err, ret)]
async fn handle(
    Extension(req_ctx): Extension<RequestContext>,
    Path(transport_service_id): Path<Id>,
    headers: HeaderMap,
    input: String,
) -> NotifyExchangeResult<()> {
    let Some(req_token) = headers.get(SECRET_TOKEN_HEADER) else {
        tracing::warn!("Missing required header: {}", SECRET_TOKEN_HEADER);
        return Err(error::invalid_request("Missing required header"));
    };

    let context = &req_ctx.global;

    let telegram_service = TelegramService::new(context);
    let transport_service = match telegram_service
        .webhook_auth(transport_service_id, req_token.as_bytes())
        .await
    {
        Ok(ts) => ts,
        Err(NotifyExchangeError::NotFound { message }) => {
            // return ok, so the message won't be resent
            tracing::warn!("Unable to found transport service: {message}");
            return Ok(());
        }
        Err(e) => return Err(e),
    };
    let token = telegram_service
        .find_token_string(context.db(), transport_service_id)
        .await?;
    let bot = telegram::new_bot(&token)?;

    // From teloxide axum_no_setup
    match serde_json::from_str::<Update>(&input) {
        Ok(mut update) => {
            // See HACK comment in
            // `teloxide_core::net::request::process_response::{closure#0}`
            if let UpdateKind::Error(_) = &mut update.kind {
                // New type
                tracing::warn!("Maybe a new type of update: {input}");
            } else {
                let chat_id = update.chat_id();
                if let Err(e) =
                    handle_update(context, &telegram_service, &transport_service, &bot, update)
                        .await
                {
                    match e {
                        NotifyExchangeError::InvalidRequest { message } => {
                            tracing::error!("Invalid request: {message}");
                            if let Some(chat_id) = chat_id {
                                bot.send_message(chat_id, message).await?;
                            }
                        }
                        NotifyExchangeError::NotFound { message } => {
                            tracing::error!("Not found: {message}");
                            if let Some(chat_id) = chat_id {
                                bot.send_message(chat_id, message).await?;
                            }
                        }
                        _ => {
                            tracing::error!("Error in handling update: {e:?}");
                            if let Some(chat_id) = chat_id {
                                bot.send_message(chat_id, "Oops, something went wrong")
                                    .await?;
                            }
                        }
                    }
                }
            }
        }
        Err(error) => {
            tracing::error!(
                "Cannot parse an update.\nError: {error:?}\nValue: {input}\n\
                     This is a bug in teloxide-core, please open an issue here: \
                     https://github.com/teloxide/teloxide/issues."
            );
        }
    }

    Ok(())
}

/// Handles the update received from Telegram.
async fn handle_update(
    context: &Context,
    telegram_service: &TelegramService<'_>,
    transport_service: &TransportService,
    bot: &Bot,
    update: Update,
) -> NotifyExchangeResult<()> {
    fn invalid_request() -> NotifyExchangeError {
        error::invalid_request("Sorry, I don't know what you mean.")
    }

    match update.kind {
        // We only handle message, edited message will be ignored
        UpdateKind::Message(message) => {
            let is_group = message.chat.is_group();
            if !message.chat.is_private() && !is_group {
                tracing::debug!("Skip message not from private chat and group chat");
                return Ok(());
            }

            if message.via_bot.is_some() {
                tracing::debug!("Skip message from bot");
                return Ok(());
            }

            let Some(sender) = message.from.clone() else {
                tracing::warn!("There is no sender of the message: {message:?}");
                return Ok(());
            };

            let (user_and_endpoint, is_admin) = if let Some((user_and_endpoint, roles)) =
                telegram_service
                    .find_user_and_roles(context.db(), sender.id, transport_service.id)
                    .await?
            {
                if !user_and_endpoint.0.enabled {
                    return Err(error::invalid_request("Your account is disabled"));
                }
                (Some(user_and_endpoint), roles.iter().any(Role::is_admin))
            } else {
                (None, false)
            };

            // Get the text from the message, and use the first word as command
            if let Some(text) = message.text() {
                let mut it = text.splitn(2, ' ');
                let Some(command) = it.next() else {
                    tracing::info!("Empty message");
                    return Err(invalid_request());
                };
                let params = it.next().unwrap_or_default();

                return telegram_service
                    .dispatch_command(
                        ChatContext {
                            bot,
                            sender: &sender,
                            chat: &message.chat,
                            transport_service,
                            user_and_endpoint: user_and_endpoint.as_ref(),
                            is_admin,
                        },
                        command,
                        params,
                    )
                    .await;
            } else {
                tracing::debug!("Skip message: {message:?}");
            }
        }
        // Callback from inline keyboard
        UpdateKind::CallbackQuery(_callback_query) => {
            return Err(error::invalid_request(
                "Inline keyboard isn't supported yet",
            ));
        }
        _ => {
            tracing::debug!("Skip update: {update:?}");
        }
    }
    Err(invalid_request())
}

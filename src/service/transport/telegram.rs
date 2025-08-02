use std::{
    borrow::Cow,
    time::{Duration, Instant},
};

use chrono::Utc;
use either::Either;
use reqwest::Url;
use sea_orm::{ConnectionTrait, DatabaseTransaction, EntityName as _};
use serde::{Deserialize, Serialize};
use teloxide::{
    Bot, RequestError,
    dispatching::dialogue::GetChatId as _,
    payloads::SetWebhookSetters as _,
    prelude::Requester,
    requests::HasPayload as _,
    types::{
        Chat, ChatId, Message as TeloxideMessage, ReplyMarkup, UpdateKind, User as TeloxideUser,
        UserId,
    },
};
use tokio::time;
use uuid::Uuid;

use crate::{
    cache::Tokenable,
    context::Context,
    db::{
        custom_type::{Id, LockType, TransportServiceType},
        entity::{
            Endpoint, MaybeExpirable as _, Message, Role, Subscription, Topic, TransportService,
            TransportServiceDsl, User,
        },
    },
    dispatcher::DispatcherClient,
    error::{self, NotifyExchangeError, NotifyExchangeResult, NotifyExchangeResultExt},
    model::{
        cache::{IdCache, TelegramSetupCache},
        req::CreateEndpointRequest,
    },
    service::{
        lock::LockService,
        secret::SecretService,
        transport::{EndpointService, TransportServiceService, telegram::command::Command},
        user::UserService,
    },
    util,
};

mod command;

#[derive(Debug, Default, Serialize, Deserialize)]
struct TelegramTransportServiceOptions {
    app_name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct TelegramEndpointOptions {
    // chat_id is needed in `bot.send_message`
    chat_id: ChatId,
}

#[derive(Serialize, Deserialize)]
struct WebhookTokenCache {
    token: String,
}

impl Tokenable for WebhookTokenCache {
    fn prefix() -> Cow<'static, str> {
        "tg_webhook_token".into()
    }
}

/// Initializes a new Telegram bot with the provided token
pub fn new_bot(token: &str) -> NotifyExchangeResult<Bot> {
    let client = teloxide::net::default_reqwest_settings()
        .connect_timeout(Duration::from_secs(15))
        .build()?;
    Ok(Bot::with_client(token, client))
}

/// Initializes a new Telegram bot with the provided token, and tests the token by calling
/// `get_me`.
pub async fn init_bot(token: &str) -> NotifyExchangeResult<Bot> {
    let bot = new_bot(token)?;
    let _ = bot.get_me().await?;
    Ok(bot)
}

#[derive(Clone, Copy)]
pub struct ChatContext<'a> {
    pub bot: &'a Bot,
    pub sender: &'a TeloxideUser,
    pub chat: &'a Chat,
    pub transport_service: &'a TransportService,
    pub user_and_endpoint: Option<&'a (User, Endpoint)>,
    pub is_admin: bool,
}

impl ChatContext<'_> {
    pub async fn send_message<M>(&self, msg: M) -> NotifyExchangeResult<()>
    where
        M: Into<String>,
    {
        let _ = self.bot.send_message(self.chat.id, msg).await?;
        Ok(())
    }

    pub async fn send_message_with_reply_markup<M>(
        &self,
        msg: M,
        reply_markup: ReplyMarkup,
    ) -> NotifyExchangeResult<()>
    where
        M: Into<String>,
    {
        let mut req = self.bot.send_message(self.chat.id, msg);
        req.reply_markup = Some(reply_markup);
        let _ = req.await?;
        Ok(())
    }

    pub fn is_public(&self) -> bool {
        self.chat.is_group()
    }
}

pub struct TelegramService<'a> {
    context: &'a Context,
    endpoint_service: EndpointService<'a>,
    lock_service: LockService<'a>,
    user_service: UserService<'a>,
    secret_service: SecretService<'a>,
    transport_service_service: TransportServiceService<'a>,
}

impl<'a> TelegramService<'a> {
    pub const WEBHOOK_PATH: &'static str = "/v1/webhook/telegram";

    pub const TS_SECRET_TYPE_TG_SECRET: &'static str = "tg-secret";

    pub fn new(context: &'a Context) -> Self {
        Self {
            context,
            endpoint_service: context.endpoint_service(),
            lock_service: context.lock_service(),
            user_service: context.user_service(),
            secret_service: context.secret_service(),
            transport_service_service: context.transport_service_service(),
        }
    }

    fn transport_service_secret_code(&self, transport_service_id: Id) -> String {
        format!("ts-{}-tg-token", transport_service_id)
    }

    #[allow(clippy::too_many_arguments)]
    async fn create_transport_service(
        &self,
        conn: &DatabaseTransaction,
        name: String,
        description: Option<String>,
        maybe_user_id: Either<Id, Id>,
        sender: &TeloxideUser,
        message: &TeloxideMessage,
        token: &str,
    ) -> NotifyExchangeResult<()> {
        self.lock_service
            .lock(conn, TransportServiceDsl.table_name(), LockType::Table)
            .await?;

        let user = match maybe_user_id {
            Either::Left(pending_user_id) => {
                // Confirm the pending user
                let (user, _) = self
                    .user_service
                    .confirm_pending_user(conn, pending_user_id)
                    .await?;
                user
            }
            Either::Right(user_id) => {
                // Get the user by id
                self.user_service.find_user_by_id(conn, user_id).await?
            }
        };

        // Create a new transport service
        let transport_service = self
            .create_telegram_transport_service(conn, name, description, &user, token)
            .await?;

        // Create a endpoint binding the telegram user. This one is for authentication.
        let create_endpoint_request = CreateEndpointRequest {
            code: Cow::Owned(user_endpoint_code(sender.id)),
            name: Cow::Owned(sender.full_name()),
            transport_service_id: transport_service.id,
            transport_service_type: transport_service.transport_service_type,
            description: Some(Cow::Owned(format!(
                "Binding while setting up Telegram transport service, chat: {:?}",
                message.chat
            ))),
            options: Self::serialize_endpoint_options(&TelegramEndpointOptions {
                chat_id: message.chat.id,
            })?,
            is_public: false,
        };
        self.endpoint_service
            .insert_endpoints(conn, user.id, vec![create_endpoint_request])
            .await?;

        // Start the service
        self.start(conn, &transport_service).await?;

        tracing::info!(
            "A telegram service[{}] by user[{}] is created, code: {:?}",
            transport_service.id,
            transport_service.user_id,
            message.text(),
        );
        Ok(())
    }

    async fn create_telegram_transport_service(
        &self,
        conn: &DatabaseTransaction,
        name: String,
        description: Option<String>,
        user: &User,
        token: &str,
    ) -> NotifyExchangeResult<TransportService> {
        let now = Utc::now();

        let transport_service = TransportService {
            id: Id::new(),
            name,
            description,
            enabled: true,
            transport_service_type: TransportServiceType::Telegram,
            user_id: user.id,
            options: Self::serialize_transport_service_options(&Default::default())?,
            created_at: now,
            updated_at: now,
        };

        let secret = self
            .secret_service
            .insert_secret(
                conn,
                &self.transport_service_secret_code(transport_service.id),
                token.as_bytes(),
                None,
                Some("telegram_bot_token".to_string()),
            )
            .await?;

        let transport_service = self
            .transport_service_service
            .insert_transport_service(
                conn,
                transport_service,
                &[(Self::TS_SECRET_TYPE_TG_SECRET, secret)],
            )
            .await?;

        Ok(transport_service)
    }

    async fn find_token<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        transport_service_id: Id,
    ) -> NotifyExchangeResult<Vec<u8>> {
        let transport_service_secrets = self
            .transport_service_service
            .find_secrets_by_transport_service_id(
                conn,
                transport_service_id,
                Self::TS_SECRET_TYPE_TG_SECRET,
            )
            .await?;

        let mut secrets = transport_service_secrets
            .into_iter()
            .filter_map(|(m, s)| {
                s.filter(|_| m.enabled && !m.is_expired())
                    .map(|s| (m.version, s))
            })
            .collect::<Vec<_>>();

        // Use oldest one
        secrets.sort_by_key(|(version, _)| *version);
        let Some((_, secret)) = secrets.first() else {
            tracing::warn!(
                "No secret found the transport service: {}",
                transport_service_id
            );
            return Err(error::not_found("Telegram token not found"));
        };

        self.secret_service.decrypt_secret(conn, secret).await
    }

    pub async fn find_token_string<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        transport_service_id: Id,
    ) -> NotifyExchangeResult<String> {
        let token = self.find_token(conn, transport_service_id).await?;
        String::from_utf8(token).whatever("Telegram token is not a valid utf8 string")
    }

    pub async fn webhook_auth(
        &self,
        transport_service_id: Id,
        req_token: &[u8],
    ) -> NotifyExchangeResult<TransportService> {
        /// All auth error return this error
        fn transport_service_not_found() -> NotifyExchangeError {
            error::not_found("Transport service not found")
        }

        // Check if the service is enabled and of type Telegram
        let transport_service = self
            .transport_service_service
            .find_by_id(self.context.db(), transport_service_id)
            .await?
            .filter(|s| {
                let res = s.enabled && s.transport_service_type == TransportServiceType::Telegram;
                if !res {
                    tracing::warn!(
                        "TelegramTransportService[{transport_service_id}]: enabled[{}], type[{:?}]",
                        s.enabled,
                        s.transport_service_type,
                    );
                }
                res
            })
            .ok_or_else(transport_service_not_found)?;

        let webhook_token = self
            .context
            .cache_manager()
            .get_token::<WebhookTokenCache>(&transport_service_id.to_string())
            .await?;
        // Check if the secret token matches
        if webhook_token
            .as_ref()
            .filter(|t| t.token.as_bytes() == req_token)
            .is_none()
        {
            tracing::warn!(
                "Token not matched, transport service: {}, has cached: [{}]",
                transport_service.id,
                webhook_token.is_some(),
            );
            return Err(transport_service_not_found());
        }
        Ok(transport_service)
    }

    fn gen_webhook_url(&self, transport_service_id: Id) -> String {
        format!(
            "{}{}/{transport_service_id}",
            self.context.config().base_api_url(),
            Self::WEBHOOK_PATH,
        )
    }

    pub async fn start<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        transport_service: &TransportService,
    ) -> NotifyExchangeResult<()> {
        let token = self.find_token_string(conn, transport_service.id).await?;
        let bot = init_bot(&token).await?;
        let webhook_url = self.gen_webhook_url(transport_service.id);
        let webhook_token = Uuid::new_v4().to_string();
        self.context
            .cache_manager()
            .set_named_token(
                &transport_service.id.to_string(),
                &WebhookTokenCache {
                    token: webhook_token.clone(),
                },
                None,
            )
            .await?;
        tracing::debug!(
            "Setting webhook url[{}] for service: {}",
            webhook_url,
            transport_service.id
        );
        bot.set_webhook(
            webhook_url
                .parse()
                .whatever(format!("Invalid telegram webhook url: {webhook_url}"))?,
        )
        .secret_token(webhook_token)
        .await?;
        Ok(())
    }

    pub async fn stop<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        transport_service: &TransportService,
    ) -> NotifyExchangeResult<()> {
        let token = self.find_token_string(conn, transport_service.id).await?;
        let bot = new_bot(&token)?;
        bot.delete_webhook().await?;
        Ok(())
    }

    #[allow(unused)]
    fn extract_endpoint_options(options: &str) -> NotifyExchangeResult<TelegramEndpointOptions> {
        serde_json::from_str(options).map_err(error::serde_json_error_cb(
            "Invalid telegram endpoint options",
        ))
    }

    fn serialize_endpoint_options(
        options: &TelegramEndpointOptions,
    ) -> NotifyExchangeResult<String> {
        serde_json::to_string(options).map_err(error::serde_json_error_cb(
            "Unable to serialize TelegramEndpointOptions",
        ))
    }

    fn extract_transport_service_options(
        options: &str,
    ) -> NotifyExchangeResult<TelegramTransportServiceOptions> {
        serde_json::from_str(options).map_err(error::serde_json_error_cb(
            "Invalid telegram transport service options",
        ))
    }

    fn serialize_transport_service_options(
        options: &TelegramTransportServiceOptions,
    ) -> NotifyExchangeResult<String> {
        serde_json::to_string(options).map_err(error::serde_json_error_cb(
            "Unable to serialize TelegramTransportServiceOptions",
        ))
    }

    pub async fn find_user_and_roles<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        telegram_user_id: UserId,
        transport_service_id: Id,
    ) -> NotifyExchangeResult<Option<((User, Endpoint), Vec<Role>)>> {
        let user_endpoint_code = user_endpoint_code(telegram_user_id);
        let Some(endpoint) = self
            .endpoint_service
            .find_by_uniq_key(conn, &user_endpoint_code, transport_service_id)
            .await?
        else {
            return Ok(None);
        };

        let user_and_roles = self
            .user_service
            .find_user_and_roles_by_id(conn, endpoint.user_id)
            .await?;
        Ok(user_and_roles.map(|(user, roles)| ((user, endpoint), roles)))
    }

    async fn find_or_create_chat_endpoint(
        &self,
        conn: &DatabaseTransaction,
        chat_ctx: &ChatContext<'_>,
        user: &User,
    ) -> NotifyExchangeResult<Endpoint> {
        // Check whether there is an endpoint for this chat. If not, create one.
        let endpoint_code = chat_endpoint_code(chat_ctx.chat.id);
        let endpoint = if let Some(endpoint) = self
            .context
            .endpoint_service()
            .find_by_uniq_key(conn, &endpoint_code, chat_ctx.transport_service.id)
            .await?
        {
            endpoint
        } else {
            self.context
                .endpoint_service()
                .insert_endpoints(
                    conn,
                    user.id,
                    vec![CreateEndpointRequest {
                        code: Cow::Owned(endpoint_code.clone()),
                        name: Cow::Owned(
                            chat_ctx
                                .chat
                                .title()
                                .map(ToString::to_string)
                                .unwrap_or_else(|| {
                                    format!("Private chat with {}", chat_ctx.sender.full_name())
                                }),
                        ),
                        transport_service_id: chat_ctx.transport_service.id,
                        transport_service_type: chat_ctx.transport_service.transport_service_type,
                        options: TelegramService::serialize_endpoint_options(
                            &TelegramEndpointOptions {
                                chat_id: chat_ctx.chat.id,
                            },
                        )?,
                        description: None,
                        is_public: !chat_ctx.chat.is_private(),
                    }],
                )
                .await?;
            let Some(endpoint) = self
                .context
                .endpoint_service()
                .find_by_uniq_key(conn, &endpoint_code, chat_ctx.transport_service.id)
                .await?
            else {
                tracing::error!(
                    "Endpoint[{}] under TransportService[{}] is inserted but not found.",
                    endpoint_code,
                    chat_ctx.transport_service.id
                );
                return Err(error::internal_server_error(
                    "Endpoint not found after inserted",
                ));
            };
            endpoint
        };
        Ok(endpoint)
    }

    async fn get_id_by_token(&self, token: &str) -> NotifyExchangeResult<Id> {
        let Some(id) = self
            .context
            .cache_manager()
            .get_token::<IdCache>(token)
            .await?
        else {
            return Err(error::invalid_request("Invalid token {token}"));
        };
        Ok(id.0)
    }

    async fn add_id_token(&self, id: Id) -> NotifyExchangeResult<String> {
        self.context
            .cache_manager()
            .add_token(&IdCache(id), Duration::from_secs(600), 10)
            .await
    }

    async fn start_app_with_url(
        &self,
        endpoint: &Endpoint,
        redirect_url: Option<String>,
    ) -> NotifyExchangeResult<Url> {
        let mut url = self.context.config().base_url().clone();
        util::extend_url(
            &mut url,
            self.context.config().web_paths().login_telegram_path(),
        )?;
        if let Some(redirect_url) = redirect_url {
            url.query_pairs_mut().append_pair("ru", &redirect_url);
        }
        url.query_pairs_mut()
            .append_pair("eid", &endpoint.id.to_string());
        tracing::debug!("start app url: {url}");
        Ok(url)
    }

    pub async fn check_endpoint_subscribable<Conn: ConnectionTrait>(
        &self,
        _conn: &Conn,
        endpoint: &Endpoint,
    ) -> NotifyExchangeResult<()> {
        if is_user_endpoint_code(&endpoint.code) {
            return Err(error::invalid_request(
                "This endpoint is for auth only in telegram service, it can't be used to subscribe",
            ));
        }
        Ok(())
    }

    pub async fn check_endpoint_before_deletion(
        &self,
        _conn: &DatabaseTransaction,
        endpoint: &Endpoint,
    ) -> NotifyExchangeResult<()> {
        if is_user_endpoint_code(&endpoint.code) {
            return Err(error::conflict(
                "This endpoint is critical for telegram service, it can't be deleted",
            ));
        }
        Ok(())
    }
}

impl<'a> TelegramService<'a> {
    const LIMIT: usize = 10;

    #[tracing::instrument(
        level = "info",
        skip(self, chat_ctx, params),
        fields(sender = %chat_ctx.sender.full_name(), chat_id = %chat_ctx.chat.id.0, is_admin = %chat_ctx.is_admin),
        err,
        ret
    )]
    pub async fn dispatch_command(
        &self,
        chat_ctx: ChatContext<'_>,
        command: &str,
        params: &str,
    ) -> NotifyExchangeResult<()> {
        let Ok(command) = command.parse::<&Command>() else {
            tracing::info!("Unknown command: {command}");
            return Err(error::invalid_request("Unknown command"));
        };
        command.execute(self.context, self, &chat_ctx, params).await
    }
}

pub struct TelegramDispatcherClient {
    context: Context,
    bot: Bot,
    topic: Topic,
    chat_id: ChatId,
}

impl TelegramDispatcherClient {
    pub(super) async fn new(
        context: &Context,
        transport_service: &TransportService,
        endpoint: &Endpoint,
        subscription: &Subscription,
    ) -> NotifyExchangeResult<Option<Self>> {
        // Make sure it is a telegram service.
        TransportServiceService::check_relations(
            transport_service,
            endpoint,
            subscription,
            TransportServiceType::Telegram,
        )?;

        let telegram_service = TelegramService::new(context);
        let token = telegram_service
            .find_token_string(context.db(), transport_service.id)
            .await?;
        let bot = new_bot(&token)?;
        let chat_id = chat_id(&endpoint.code)?;
        let topic = context
            .topic_service()
            .find_by_id(context.db(), subscription.topic_id)
            .await?;

        Ok(Some(Self {
            context: context.clone(),
            bot,
            topic,
            chat_id,
        }))
    }
}

impl DispatcherClient for TelegramDispatcherClient {
    async fn dispatch<F>(
        &mut self,
        message: &Message,
        data: Vec<u8>,
        has_changed: F,
    ) -> Result<(), bool>
    where
        F: Fn() -> bool,
    {
        let sender = self
            .context
            .user_service()
            .find_user_by_id(self.context.db(), message.user_id)
            .await
            .map(|user| format!("{} - {}", user.username, user.id))
            .unwrap_or_else(|_| format!("{}", message.user_id));

        let formatted = format!(
            "Topic: {} - {}\nSender: {}\nMessage: {}",
            self.topic.name,
            self.topic.id,
            sender,
            String::from_utf8_lossy(&data)
        );

        let mut count = 0;
        let mut chat_id = self.chat_id;
        loop {
            let timeout;
            match self.bot.send_message(chat_id, &formatted).await {
                Ok(_) => return Ok(()),
                Err(e) => match e {
                    RequestError::Api(api_error) => {
                        tracing::error!("Api error of Telegram: {api_error:#?}");
                        // Don't retry after api error
                        return Err(false);
                    }
                    RequestError::MigrateToChatId(new_chat_id) => {
                        tracing::debug!(
                            "Telegram Chat[{chat_id}] has been migrated to Chat[{new_chat_id}]"
                        );
                        chat_id = new_chat_id;
                        continue;
                    }
                    RequestError::RetryAfter(seconds) => {
                        tracing::info!("Retry sending to Telegram after {seconds}");
                        timeout = seconds.duration();
                    }
                    RequestError::Network(error) => {
                        tracing::warn!("Network error of sending to Telegram: {error:?}");
                        timeout = Duration::from_secs(5 * (1 << count));
                    }
                    RequestError::InvalidJson { source, raw } => {
                        tracing::error!(
                            "Invalid json of Telegram request: {source:#?}, request: {raw}"
                        );
                        // Don't retry after invalid json
                        return Err(false);
                    }
                    RequestError::Io(error) => {
                        tracing::error!("Io error of sending to Telegram: {error:#?}");
                        timeout = Duration::from_secs(5 * (1 << count));
                    }
                },
            }
            count += 1;
            if has_changed() {
                return Err(false);
            }
            if count > 10 {
                return Err(false);
            }
            time::sleep(timeout).await
        }
    }
}

/// Create a new async task to finish the setup of a telegram transport service using polling api.
///
/// Params:
/// - `maybe_user_id`: It maybe a pending user id or a user id, depending on the state of the user.
#[tracing::instrument(level = "info", skip(context, bot), err, ret)]
pub async fn wait_and_finish_setup(
    context: Context,
    bot: Bot,
    name: String,
    description: Option<String>,
    maybe_user_id: Either<Id, Id>,
    token: String,
    timeout: Duration,
) -> NotifyExchangeResult<bool> {
    // make sure there is no webhook set, otherwise, `get_updates` will failed
    bot.delete_webhook().await?;

    let begin = Instant::now();

    let mut offset = None;
    let message;
    let sender;
    'outer: loop {
        let now = Instant::now();
        if now.duration_since(begin) > timeout {
            tracing::warn!("Timeout reached while waiting for Telegram setup to finish");
            return Ok(false);
        }
        let updates = bot
            .get_updates()
            .with_payload_mut(|payload| {
                payload.timeout = Some(10);
                payload.offset = offset;
            })
            .await?;
        for update in updates {
            offset = Some(update.id.as_offset());
            if let UpdateKind::Message(m) = update.kind {
                if m.via_bot.is_some() {
                    continue;
                }
                if m.text() == Some(&token) {
                    // Skip message which has no sender
                    let Some(s) = m.from.clone() else {
                        continue;
                    };
                    message = m;
                    sender = s;
                    break 'outer;
                }
            }
        }
    }

    // Commit the offset
    bot.get_updates()
        .with_payload_mut(|payload| {
            payload.offset = offset;
            payload.limit = Some(1);
        })
        .await?;

    let res = context
        .transaction(async |conn| {
            TelegramService::new(&context)
                .create_transport_service(
                    conn,
                    name,
                    description,
                    maybe_user_id,
                    &sender,
                    &message,
                    bot.token(),
                )
                .await
        })
        .await;
    let success = res.is_ok();
    let cache_res = if success {
        context
            .cache_manager()
            .set_named_token(&token, &TelegramSetupCache { created: true }, Some(timeout))
            .await
    } else {
        context
            .cache_manager()
            .delete_token::<TelegramSetupCache>(&token)
            .await
    };
    if let Err(e) = cache_res {
        tracing::error!("Failed to update/delete telegram setup cache: {e:?}");
    }
    let text = match res {
        Ok(_) => "Telegram service registered",
        Err(_) => "Error happened",
    };
    if let Some(chat_id) = message.chat.chat_id() {
        bot.send_message(chat_id, text).await?;
    }
    Ok(true)
}

fn is_user_endpoint_code(code: &str) -> bool {
    code.starts_with("user-")
}

pub fn user_endpoint_code(user_id: UserId) -> String {
    format!("user-{}", user_id.0)
}

#[allow(unused)]
fn user_id(endpoint_code: &str) -> NotifyExchangeResult<UserId> {
    if let Some(id) = endpoint_code.strip_prefix("user-")
        && let Ok(id) = id.parse()
    {
        return Ok(UserId(id));
    }
    Err(error::internal_server_error(format!(
        "Invalid telegram user endpoint_code: {endpoint_code}"
    )))
}

fn chat_endpoint_code(chat_id: ChatId) -> String {
    format!("chat-{}", chat_id.0)
}

fn chat_id(endpoint_code: &str) -> NotifyExchangeResult<ChatId> {
    if let Some(id) = endpoint_code.strip_prefix("chat-")
        && let Ok(id) = id.parse()
    {
        return Ok(ChatId(id));
    }
    Err(error::internal_server_error(format!(
        "Invalid telegram chat endpoint_code: {endpoint_code}"
    )))
}

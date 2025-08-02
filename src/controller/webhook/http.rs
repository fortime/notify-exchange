use std::{
    sync::{
        Arc,
        atomic::{AtomicIsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Extension, Json, Router,
    extract::Query,
    http::{Extensions, HeaderMap, Uri, uri::Scheme},
    middleware, routing,
};
use base64::{Engine as _, prelude::BASE64_STANDARD};
use bytes::Bytes;
use rsa::pkcs8::der::zeroize::Zeroizing;
use sea_orm::{ActiveModelTrait, ConnectionTrait, IntoActiveModel};
use serde::{Deserialize, Serialize};

use crate::{
    auth::{self, HttpApiCalleeAuthHelper, SignAlgo},
    config::secret::Key,
    context::{Context, RequestContext},
    controller::MaybeSuccessResponse,
    db::{
        custom_type::{Id, TransportServiceType},
        entity::{Endpoint, EndpointSecret, MaybeExpirable as _},
    },
    error::{self, NotifyExchangeResult},
    service::{
        secret::{self, AesGcmOptions, SecretService},
        transport::{EndpointService, http::HttpService},
    },
};

pub fn route<S>(router: Router<S>, context: &Context) -> Router<S>
where
    S: 'static + Clone + Sync + Send,
{
    let sub_router = Router::<S>::new()
        .route(
            "/webhook",
            routing::post(set_webhook).delete(delete_webhook),
        )
        .route("/message", routing::post(publish_message).get(pull_message))
        .route("/offset", routing::post(commit_offset))
        .route(
            "/endpoint/encrypt-secret",
            routing::post(refresh_encrypt_secret),
        )
        .layer(middleware::from_fn_with_state(
            HttpTransportEndpointAuthHelper::new(context),
            auth::http_api_auth_middleware::<HttpTransportEndpointAuthHelper>,
        ));
    router.nest(HttpService::WEBHOOK_PATH, sub_router)
}

#[derive(Clone)]
struct AuthContext {
    endpoint: Arc<Endpoint>,
    pub_keys: Arc<Vec<(EndpointSecret, Bytes)>>,
    verified_public_key_index: Arc<AtomicIsize>,
}

impl AuthContext {
    /// Get the code and data of the public key code which is used for auth.
    fn verified_public_key(&self) -> NotifyExchangeResult<(&str, &Bytes)> {
        let index = self.verified_public_key_index.load(Ordering::SeqCst);
        let index = if index < 0 {
            return Err(error::internal_server_error(
                "There is no verified public key",
            ));
        } else {
            index as usize
        };
        let Some((secret, data)) = self.pub_keys.get(index) else {
            return Err(error::internal_server_error(
                "The index of verified public key is out of range",
            ));
        };
        Ok((&secret.code, data))
    }
}

#[derive(Clone)]
pub struct HttpTransportEndpointAuthHelper {
    context: Context,
}

impl HttpTransportEndpointAuthHelper {
    pub fn new(context: &Context) -> Self {
        Self {
            context: context.clone(),
        }
    }

    fn auth_ctx(extensions: &Extensions) -> NotifyExchangeResult<&AuthContext> {
        let Some(auth_ctx) = extensions.get::<AuthContext>() else {
            tracing::error!("AutthContext is not set");
            return Err(error::internal_server_error("AutthContext is not set"));
        };
        Ok(auth_ctx)
    }
}

impl HttpApiCalleeAuthHelper for HttpTransportEndpointAuthHelper {
    fn context(&self) -> &Context {
        &self.context
    }

    async fn extend(
        &self,
        extensions: &mut Extensions,
        biz_code: &str,
        _headers: &HeaderMap,
    ) -> NotifyExchangeResult<()> {
        // biz_code is the id of a endpoint
        let endpoint_id: Id = biz_code.parse()?;
        let endpoint_service = self.context.endpoint_service();
        let Some((endpoint, pub_keys)) = endpoint_service
            .find_endpoint_and_secrets_by_id(
                self.context.db(),
                endpoint_id,
                HttpService::E_SECRET_TYPE_AUTH_PUB_KEY,
                None,
            )
            .await?
        else {
            tracing::debug!("Endpoint does not exist: {endpoint_id}");
            return Err(error::invalid_sign("Endpoint doest not exist"));
        };

        let Some(transport_service) = self
            .context
            .transport_service_service()
            .find_by_id(self.context.db(), endpoint.transport_service_id)
            .await?
            .filter(|s| s.enabled)
        else {
            tracing::debug!(
                "Transport service does not exist or is not enabled: {}",
                endpoint.transport_service_id
            );
            return Err(error::invalid_sign(
                "Transport service does not exist or is not enabled",
            ));
        };

        if transport_service.transport_service_type != TransportServiceType::Http {
            return Err(error::invalid_sign("Transport service is not http"));
        }

        let pub_keys = endpoint_service
            .decrypt_endpoint_secrets(self.context.db(), pub_keys)
            .await?;

        extensions.insert(AuthContext {
            endpoint: Arc::new(endpoint),
            pub_keys: Arc::new(pub_keys),
            verified_public_key_index: Arc::new(AtomicIsize::new(-1)),
        });
        Ok(())
    }

    async fn set_verified_public_key(
        &self,
        extensions: &Extensions,
        index: usize,
    ) -> NotifyExchangeResult<()> {
        let auth_ctx = Self::auth_ctx(extensions)?;
        auth_ctx
            .verified_public_key_index
            .store(index as isize, Ordering::SeqCst);
        Ok(())
    }

    async fn public_keys<'a>(
        &'a self,
        extensions: &'a Extensions,
        biz_code: &'a str,
        sign_algo: SignAlgo,
    ) -> NotifyExchangeResult<Vec<&'a Bytes>> {
        let auth_ctx = Self::auth_ctx(extensions)?;
        match sign_algo {
            SignAlgo::Sha256Rsa => {
                let res: Vec<_> = auth_ctx
                    .pub_keys
                    .iter()
                    .filter_map(|(s, bs)| if s.is_expired() { None } else { Some(bs) })
                    .collect();
                if res.is_empty() {
                    return Err(error::invalid_sign(format!(
                        "There is no valid public key set: {biz_code}"
                    )));
                }
                Ok(res)
            }
        }
    }

    async fn response_biz_code(
        &self,
        _biz_code: &str,
        _sign_algo: SignAlgo,
    ) -> NotifyExchangeResult<String> {
        Ok(HttpService::SERVER_BIZ_CODE.to_string())
    }

    async fn private_key(
        &self,
        biz_code: &str,
        sign_algo: SignAlgo,
    ) -> NotifyExchangeResult<Zeroizing<Vec<u8>>> {
        if biz_code != HttpService::SERVER_BIZ_CODE {
            return Err(error::invalid_sign("Unknown response biz code"));
        }
        match sign_algo {
            SignAlgo::Sha256Rsa => {
                let Some(key) = self
                    .context
                    .secret_service()
                    .find_key_by_code_from_db(self.context.db(), SecretService::AUTH_RSA_KEY_CODE)
                    .await?
                else {
                    return Err(error::internal_server_error("Auth rsa key not found"));
                };
                match key {
                    Key::Rsa { pri_key, .. } => {
                        let pri_key = BASE64_STANDARD.decode(pri_key).map_err(|e| {
                            tracing::warn!("RSA private key is not base64 encoded: {:?}", e);
                            error::internal_server_error("RSA private key is not base64 encoded")
                        })?;
                        Ok(pri_key.into())
                    }
                    _ => Err(error::internal_server_error("Auth rsa key is not rsa key")),
                }
            }
        }
    }
}

#[derive(Deserialize, Debug)]
struct SetWebhookRequest {
    url: String,
    encrypt_secret_code_in_push: String,
}

#[derive(Deserialize)]
struct PublishMessageRequest {
    topic_code: String,
    topic_user_id: Option<Id>,
    encrypted_message: String,
    nonce: String,
    encrypt_secret_code: String,
}

#[derive(Deserialize)]
struct PullMessageQuery {
    topic_code: String,
    topic_user_id: Option<Id>,
    /// The most number of message will be return, max: 255.
    limit: u8,
    /// The second of poll timeout.
    poll_timeout: Option<u8>,
    /// The code of the secret used to encrypt the message.
    encrypt_secret_code: String,
}

#[derive(Serialize)]
struct MessageAndOffset {
    data: String,
    offset: i64,
}

#[derive(Serialize)]
struct PullMessageResponse {
    sub_id: Id,
    encrypted_messages: Vec<MessageAndOffset>,
    nonce: String,
}

#[derive(Deserialize)]
struct CommitOffsetRequest {
    sub_id: Id,
    offset: i64,
}

#[derive(Deserialize)]
struct RefreshEncryptSecretRequest {
    code: String,
    /// if force is false, no new secret will be created when the current version is created in the
    /// past one hour.
    force: bool,
}

#[derive(Serialize)]
struct RefreshEncryptSecretResponse {
    /// Base64 encoded RSA encrypted secret, if no secret created, it will be null.
    encrypted_secret: Option<String>,
}

fn base64_decode(s: &str) -> NotifyExchangeResult<Vec<u8>> {
    BASE64_STANDARD
        .decode(s)
        .map_err(|_| error::invalid_request("Invalid base64 encoding"))
}

async fn update_endpoint_options(
    context: &Context,
    endpoint: &Endpoint,
    url: Option<String>,
    encrypt_secret_code_in_push: Option<String>,
) -> NotifyExchangeResult<()> {
    if let Some(code) = &encrypt_secret_code_in_push
        && context
            .endpoint_service()
            .find_secrets_by_id(
                context.db(),
                endpoint.id,
                HttpService::E_SECRET_TYPE_ENCRYPT_SECRET,
                Some(code),
                Some(true),
                false,
            )
            .await?
            .is_empty()
    {
        tracing::debug!("No encrypt secret[{code}] of endpoint[{}]", endpoint.id);
        return Err(error::invalid_request("No such encrypt-secret"));
    }

    let options = HttpService::extract_endpoint_options(&endpoint.options)?
        .set_url(url)
        .set_esc_in_push(encrypt_secret_code_in_push);
    let mut endpoint = endpoint.clone().into_active_model();
    endpoint
        .options
        .set_if_not_equals(HttpService::serialize_endpoint_options(&options)?);
    context
        .transaction(async |conn| {
            let endpoint = endpoint.update(conn).await?;
            context
                .subscription_service()
                .reset_failed_subscription_of_endpoint(conn, &endpoint)
                .await?;
            context
                .dispatcher_manager()
                .refresh_by_endpoint(endpoint.id);
            Ok(())
        })
        .await?;
    Ok(())
}

#[tracing::instrument(level = "info", skip(req_ctx, auth_ctx), err)]
async fn set_webhook(
    Extension(req_ctx): Extension<RequestContext>,
    Extension(auth_ctx): Extension<AuthContext>,
    Json(req): Json<SetWebhookRequest>,
) -> MaybeSuccessResponse<()> {
    let url = req
        .url
        .parse::<Uri>()
        .map_err(|_| error::invalid_request("Invalid url"))?;
    if url
        .scheme()
        .filter(|s| **s == Scheme::HTTP || **s == Scheme::HTTPS)
        .is_none()
    {
        return Err(error::invalid_request("Invalid scheme"));
    }
    if url.authority().is_none() {
        return Err(error::invalid_request("Invalid authority"));
    }
    update_endpoint_options(
        &req_ctx.global,
        &auth_ctx.endpoint,
        Some(url.to_string()),
        Some(req.encrypt_secret_code_in_push),
    )
    .await
    .map(Into::into)
}

#[tracing::instrument(level = "info", skip_all, err)]
async fn delete_webhook(
    Extension(req_ctx): Extension<RequestContext>,
    Extension(auth_ctx): Extension<AuthContext>,
) -> MaybeSuccessResponse<()> {
    tracing::info!(
        "Delete webhook options from endpoint[{}]",
        auth_ctx.endpoint.id
    );
    update_endpoint_options(&req_ctx.global, &auth_ctx.endpoint, None, None)
        .await
        .map(Into::into)
}

async fn find_encrypt_secret<Conn: ConnectionTrait>(
    conn: &Conn,
    endpoint_service: EndpointService<'_>,
    endpoint_id: Id,
    code: &str,
) -> NotifyExchangeResult<(EndpointSecret, Bytes)> {
    let encrypt_secrets = endpoint_service
        .find_secrets_by_id(
            conn,
            endpoint_id,
            HttpService::E_SECRET_TYPE_ENCRYPT_SECRET,
            Some(code),
            Some(true),
            false,
        )
        .await?;
    // use the latest one
    if let Some(encrypt_secret) = endpoint_service
        .decrypt_endpoint_secrets(conn, encrypt_secrets)
        .await?
        .pop()
    {
        Ok(encrypt_secret)
    } else {
        tracing::warn!(
            "There is no valid encrypt secret[{code}] for http endpoint[{}]",
            endpoint_id
        );
        Err(error::invalid_request("There is no encrypt secret set"))
    }
}

async fn publish_message(
    Extension(req_ctx): Extension<RequestContext>,
    Extension(auth_ctx): Extension<AuthContext>,
    Json(req): Json<PublishMessageRequest>,
) -> MaybeSuccessResponse<()> {
    let topic_user_id = req.topic_user_id.unwrap_or(auth_ctx.endpoint.user_id);
    let context = &req_ctx.global;

    let encrypt_secret = find_encrypt_secret(
        context.db(),
        context.endpoint_service(),
        auth_ctx.endpoint.id,
        &req.encrypt_secret_code,
    )
    .await?;
    let message = secret::decrypt_aes_gcm(
        &base64_decode(&req.encrypted_message)?,
        &encrypt_secret.1,
        &base64_decode(&req.nonce)?,
    )?;

    req_ctx
        .global
        .topic_service()
        .publish_by_uniq_key(
            req_ctx.global.db(),
            &req.topic_code,
            Some(topic_user_id),
            auth_ctx.endpoint.id,
            auth_ctx.endpoint.user_id,
            &message,
        )
        .await?;
    Ok(().into())
}

async fn pull_message(
    Extension(req_ctx): Extension<RequestContext>,
    Extension(auth_ctx): Extension<AuthContext>,
    Query(req): Query<PullMessageQuery>,
) -> MaybeSuccessResponse<PullMessageResponse> {
    let topic_user_id = req.topic_user_id.unwrap_or(auth_ctx.endpoint.user_id);
    let context = &req_ctx.global;
    let conn = context.db();

    let encrypt_secret = find_encrypt_secret(
        conn,
        context.endpoint_service(),
        auth_ctx.endpoint.id,
        &req.encrypt_secret_code,
    )
    .await?;

    let topic_service = context.topic_service();
    let topic = topic_service
        .find_by_uniq_key(conn, &req.topic_code, topic_user_id)
        .await?;

    let Some((_, sub_offset)) = context
        .subscription_service()
        .find_subscription_and_offset(conn, topic.id, auth_ctx.endpoint.id)
        .await?
    else {
        return Err(error::forbidden("No subscription"));
    };
    let messages = topic_service
        .pull(
            topic.id,
            sub_offset.next_message_id,
            req.limit as usize,
            Duration::from_secs(req.poll_timeout.unwrap_or_default() as u64),
        )
        .await?;

    let mut encrypted_messages = Vec::with_capacity(messages.len());
    let nonce = AesGcmOptions::new().nonce;

    if messages.is_empty() {
        // If there is no message, return encrypted_aes_key, nonce and public_key_code as empty
        // strings.
        return Ok(PullMessageResponse {
            sub_id: sub_offset.subscription_id,
            encrypted_messages,
            nonce: Default::default(),
        }
        .into());
    }

    let secret_service = context.secret_service();

    for message in messages {
        let data = secret_service.decrypt_secret(conn, &message).await?;
        encrypted_messages.push(MessageAndOffset {
            data: BASE64_STANDARD.encode(secret::encrypt_aes_gcm(
                &data,
                &encrypt_secret.1,
                &nonce,
            )?),
            offset: message.id,
        });
    }

    Ok(PullMessageResponse {
        sub_id: sub_offset.subscription_id,
        encrypted_messages,
        nonce: BASE64_STANDARD.encode(&nonce),
    }
    .into())
}

async fn commit_offset(
    Extension(req_ctx): Extension<RequestContext>,
    Extension(auth_ctx): Extension<AuthContext>,
    Json(req): Json<CommitOffsetRequest>,
) -> MaybeSuccessResponse<()> {
    req_ctx
        .global
        .subscription_service()
        .commit(
            req_ctx.global.db(),
            &auth_ctx.endpoint,
            req.sub_id,
            req.offset,
        )
        .await?;
    Ok(().into())
}

/// Create a new one if the number of encrypt secret doesn't exceed the limit or create a new
/// version and disable the old version.
async fn refresh_encrypt_secret(
    Extension(req_ctx): Extension<RequestContext>,
    Extension(auth_ctx): Extension<AuthContext>,
    Json(req): Json<RefreshEncryptSecretRequest>,
) -> MaybeSuccessResponse<RefreshEncryptSecretResponse> {
    let secret = secret::new_aes_gcm_key_data();
    let endpoint_secret = req_ctx
        .global
        .transaction(async |conn| {
            HttpService::new(&req_ctx.global)
                .refresh_encrypt_secret(
                    conn,
                    auth_ctx.endpoint.id,
                    &req.code,
                    HttpService::E_SECRET_TYPE_ENCRYPT_SECRET,
                    &secret,
                    HttpService::LIMIT_ENCRYPT_SECRET,
                    Default::default(),
                    req.force,
                )
                .await
        })
        .await?;
    if endpoint_secret.is_some() {
        let (_, public_key) = auth_ctx.verified_public_key()?;
        Ok(RefreshEncryptSecretResponse {
            encrypted_secret: Some(
                BASE64_STANDARD.encode(secret::encrypt_rsa(&secret, public_key)?),
            ),
        }
        .into())
    } else {
        Ok(RefreshEncryptSecretResponse {
            encrypted_secret: None,
        }
        .into())
    }
}

use std::{borrow::Cow, time::Duration};

use base64::{Engine as _, prelude::BASE64_STANDARD};
use bytes::Bytes;
use chrono::Utc;
use reqwest::{Client, StatusCode};
use rsa::pkcs8::der::zeroize::Zeroizing;
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, ConnectionTrait, DatabaseTransaction, EntityTrait,
    QueryFilter as _, QuerySelect, sea_query::Cond,
};
use serde::{Deserialize, Serialize};
use tokio::time;
use validator::Validate as _;

use crate::{
    auth::{HttpApiCallerAuthHelper, SignAlgo},
    config::secret::Key,
    context::Context,
    db::{
        custom_type::{Id, TransportServiceType},
        entity::{
            ActiveEndpointSecret, Endpoint, EndpointSecret, MaybeExpirable, Message, Subscription,
            TransportService, TransportServiceColumn, TransportServiceDsl, User,
        },
    },
    dispatcher::DispatcherClient,
    error::{self, NotifyExchangeError, NotifyExchangeResult},
    model::req::{CreateEndpointRequest, CreateHttpTransportServiceRequest},
    service::{
        secret::{self, AesGcmOptions, SecretService},
        transport::TransportServiceService,
    },
};

#[derive(Default, Deserialize, Serialize)]
pub struct HttpEndpointOptions {
    url: Option<String>,
    // encrypt_secret_code using in push
    esc_in_push: Option<String>,
}

impl HttpEndpointOptions {
    pub fn set_url(mut self, url: Option<String>) -> Self {
        self.url = url;
        self
    }

    pub fn set_esc_in_push(mut self, esc_in_push: Option<String>) -> Self {
        self.esc_in_push = esc_in_push;
        self
    }

    pub fn esc_in_push(&self) -> Option<&str> {
        self.esc_in_push.as_deref()
    }
}

pub struct HttpService<'a> {
    context: &'a Context,
}

impl<'a> HttpService<'a> {
    pub const WEBHOOK_PATH: &'static str = "/v1/webhook/http";

    pub const SERVER_BIZ_CODE: &'static str = "ne-v1";

    pub const E_SECRET_TYPE_AUTH_PUB_KEY: &'static str = "auth-pub-key";

    pub const E_SECRET_TYPE_ENCRYPT_SECRET: &'static str = "encrypt-secret";

    pub const LIMIT_ENCRYPT_SECRET: u64 = 3;

    pub const LIMIT_AUTH_PUB_KEY: u64 = 3;

    pub fn new(context: &'a Context) -> Self {
        Self { context }
    }

    pub fn extract_endpoint_options(options: &str) -> NotifyExchangeResult<HttpEndpointOptions> {
        serde_json::from_str(options)
            .map_err(error::serde_json_error_cb("Invalid http endpoint options"))
    }

    pub fn serialize_endpoint_options(
        options: &HttpEndpointOptions,
    ) -> NotifyExchangeResult<String> {
        serde_json::to_string(&options)
            .map_err(error::serde_json_error_cb("Invalid http endpoint options"))
    }

    pub async fn find_rsa_decrypt_key(&self, key_code: &str) -> NotifyExchangeResult<Bytes> {
        let key = if key_code == Self::SERVER_BIZ_CODE {
            self.context
                .secret_service()
                .find_key_by_code_from_db(self.context.db(), SecretService::AUTH_RSA_KEY_CODE)
                .await?
        } else {
            None
        };
        let Some(key) = key else {
            return Err(error::internal_server_error("Rsa key not found"));
        };
        match key {
            Key::Rsa { pri_key, .. } => Ok(pri_key),
            _ => Err(error::internal_server_error("Rsa key is not rsa key")),
        }
    }

    pub async fn create_transport_service(
        &self,
        conn: &DatabaseTransaction,
        user: &User,
        req: CreateHttpTransportServiceRequest<'_>,
    ) -> NotifyExchangeResult<TransportService> {
        req.validate()?;
        let CreateHttpTransportServiceRequest { name, description } = req;
        // It looks like there is no need to create multiple http service
        let transport_service =
            TransportServiceDsl::find()
                .filter(Cond::all().add(
                    TransportServiceColumn::TransportServiceType.eq(TransportServiceType::Http),
                ))
                .limit(1)
                .one(conn)
                .await?;
        if let Some(transport_service) = transport_service {
            return Ok(transport_service);
        }
        let now = Utc::now();
        let transport_service = TransportService {
            id: Id::new(),
            name: name.into_owned(),
            transport_service_type: TransportServiceType::Http,
            user_id: user.id,
            options: "{}".to_string(),
            enabled: true,
            description: description.map(Cow::into_owned),
            created_at: now,
            updated_at: now,
        };
        self.context
            .transport_service_service()
            .insert_transport_service(conn, transport_service, &[])
            .await
    }

    pub async fn create_endpoint<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        user: &User,
        transport_service_id: Id,
        code: &str,
        name: &str,
        description: Option<&str>,
    ) -> NotifyExchangeResult<Endpoint> {
        let Some(transport_service) = self
            .context
            .transport_service_service()
            .find_by_id(conn, transport_service_id)
            .await?
        else {
            return Err(error::invalid_request("No such service"));
        };
        if transport_service.transport_service_type != TransportServiceType::Http {
            tracing::debug!(
                "Transport type of {} is {:?} instead of Http",
                transport_service_id,
                transport_service.transport_service_type
            );
            return Err(error::invalid_request("No such service"));
        }
        let mut endpoints = self
            .context
            .endpoint_service()
            .insert_endpoints(
                conn,
                user.id,
                vec![CreateEndpointRequest {
                    name: Cow::Borrowed(name),
                    code: Cow::Borrowed(code),
                    transport_service_id: transport_service.id,
                    transport_service_type: transport_service.transport_service_type,
                    options: HttpService::serialize_endpoint_options(
                        &HttpEndpointOptions::default(),
                    )?,
                    description: description.map(Cow::Borrowed),
                    is_public: false,
                }],
            )
            .await?;
        if let Some(endpoint) = endpoints.pop() {
            if endpoints.is_empty() {
                return Ok(endpoint);
            }
            endpoints.push(endpoint);
        }
        Err(error::internal_server_error(format!(
            "The number of inserted endpoint should be 1, but {}.",
            endpoints.len()
        )))
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn refresh_encrypt_secret(
        &self,
        conn: &DatabaseTransaction,
        endpoint_id: Id,
        code: &str,
        secret_type: &str,
        secret: &[u8],
        limit: u64,
        op: RefreshEncryptSecretOp,
        force: bool,
    ) -> NotifyExchangeResult<Option<EndpointSecret>> {
        let endpoint_service = self.context.endpoint_service();

        // Lock the endpoint, so the dispatcher_manager can have the latest record.
        endpoint_service
            .lock_endpoint(conn, endpoint_id, true)
            .await?;

        let encrypt_secrets = endpoint_service
            .find_secrets_by_id(conn, endpoint_id, secret_type, Some(code), None, true)
            .await?;
        if let Some(last) = encrypt_secrets.last() {
            if op == RefreshEncryptSecretOp::Create {
                return Err(error::conflict("Duplicated encrypt secret code"));
            }
            if force
                || !last.0.enabled
                || last.0.is_expired()
                // Allow refresh the secret created before the past one hour
                || last.0.created_at + Duration::from_secs(3600) < Utc::now()
            {
                let endpoint_secret = endpoint_service
                    .update_endpoint_secret(
                        conn,
                        endpoint_id,
                        code,
                        secret_type,
                        secret,
                        true,
                        None,
                    )
                    .await?;
                // Disable the old one.
                ActiveEndpointSecret {
                    id: ActiveValue::Unchanged(last.0.id),
                    enabled: ActiveValue::Set(false),
                    ..Default::default()
                }
                .update(conn)
                .await?;
                // Rebuild dispatcher client
                self.context
                    .dispatcher_manager()
                    .refresh_by_endpoint(endpoint_id);
                Ok(Some(endpoint_secret))
            } else {
                Ok(None)
            }
        } else {
            if op == RefreshEncryptSecretOp::Update {
                return Err(error::not_found("No such encrypt secret code"));
            }
            if endpoint_service
                .count_endpoint_secrets_group_by_type(conn, endpoint_id, secret_type)
                .await?
                < limit
            {
                let endpoint_secret = endpoint_service
                    .insert_endpoint_secret(
                        conn,
                        endpoint_id,
                        code,
                        0,
                        secret_type,
                        secret,
                        true,
                        None,
                    )
                    .await?;
                // Rebuild dispatcher client
                self.context
                    .dispatcher_manager()
                    .refresh_by_endpoint(endpoint_id);
                Ok(Some(endpoint_secret))
            } else {
                Err(error::conflict("Too many encrypt secrets for endpoint"))
            }
        }
    }

    pub async fn start<Conn: ConnectionTrait>(
        &self,
        _conn: &Conn,
        _transport_service: &TransportService,
    ) -> NotifyExchangeResult<()> {
        Ok(())
    }

    pub async fn stop<Conn: ConnectionTrait>(
        &self,
        _conn: &Conn,
        _transport_service: &TransportService,
    ) -> NotifyExchangeResult<()> {
        Ok(())
    }

    pub async fn create_endpoint_secret(
        &self,
        conn: &DatabaseTransaction,
        endpoint: &Endpoint,
        code: Cow<'_, str>,
        secret_type: Cow<'_, str>,
        secret: Option<Bytes>,
    ) -> NotifyExchangeResult<(EndpointSecret, Bytes)> {
        if endpoint.transport_service_type != TransportServiceType::Http {
            tracing::error!("Expected Http, but {:?}", endpoint.transport_service_type);
            return Err(error::invalid_request("Invalid transport service type"));
        }
        let (limit, secret) = match secret_type.as_ref() {
            Self::E_SECRET_TYPE_AUTH_PUB_KEY => {
                let Some(secret) = secret else {
                    return Err(error::invalid_request("No public key provided"));
                };

                // Convert a pem key to a der
                (
                    Self::LIMIT_AUTH_PUB_KEY,
                    secret::pub_key_pem_to_der(&secret)?,
                )
            }
            Self::E_SECRET_TYPE_ENCRYPT_SECRET => {
                (Self::LIMIT_ENCRYPT_SECRET, secret::new_aes_gcm_key_data())
            }
            _ => {
                tracing::error!("Unsupported endpoint secret type for Http: {}", secret_type);
                return Err(error::invalid_request("Invalid endpoint secret type"));
            }
        };
        if let Some(res) = self
            .refresh_encrypt_secret(
                conn,
                endpoint.id,
                &code,
                &secret_type,
                &secret,
                limit,
                RefreshEncryptSecretOp::Create,
                false,
            )
            .await?
        {
            Ok((res, secret))
        } else {
            tracing::error!(
                "Can't create a encrypt secret for endpoint[{}]",
                endpoint.id
            );
            Err(error::internal_server_error(
                "Unable to create a encrypt secret",
            ))
        }
    }

    pub async fn update_endpoint_secret(
        &self,
        conn: &DatabaseTransaction,
        endpoint: &Endpoint,
        code: Cow<'_, str>,
        secret_type: Cow<'_, str>,
        secret: Option<Bytes>,
    ) -> NotifyExchangeResult<(EndpointSecret, Bytes)> {
        if endpoint.transport_service_type != TransportServiceType::Http {
            tracing::error!("Expected Http, but {:?}", endpoint.transport_service_type);
            return Err(error::invalid_request("Invalid transport service type"));
        }
        let (limit, secret) = match secret_type.as_ref() {
            Self::E_SECRET_TYPE_AUTH_PUB_KEY => {
                let Some(secret) = secret else {
                    return Err(error::invalid_request("No public key provided"));
                };

                // Convert a pem key to a der
                (
                    Self::LIMIT_AUTH_PUB_KEY,
                    secret::pub_key_pem_to_der(&secret)?,
                )
            }
            Self::E_SECRET_TYPE_ENCRYPT_SECRET => {
                (Self::LIMIT_ENCRYPT_SECRET, secret::new_aes_gcm_key_data())
            }
            _ => {
                tracing::error!("Unsupported endpoint secret type for Http: {}", secret_type);
                return Err(error::invalid_request("Invalid endpoint secret type"));
            }
        };
        if let Some(res) = self
            .refresh_encrypt_secret(
                conn,
                endpoint.id,
                &code,
                &secret_type,
                &secret,
                limit,
                RefreshEncryptSecretOp::Update,
                true,
            )
            .await?
        {
            Ok((res, secret))
        } else {
            tracing::error!(
                "Can't update a encrypt secret for endpoint[{}]",
                endpoint.id
            );
            Err(error::internal_server_error(
                "Unable to update the encrypt secret",
            ))
        }
    }

    pub async fn check_endpoint_subscribable<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        endpoint: &Endpoint,
    ) -> NotifyExchangeResult<()> {
        if self
            .context
            .endpoint_service()
            .find_secrets_by_id(
                conn,
                endpoint.id,
                HttpService::E_SECRET_TYPE_AUTH_PUB_KEY,
                None,
                Some(true),
                false,
            )
            .await?
            .is_empty()
        {
            return Err(error::conflict(
                "There is no valid auth pub key of this http endpoint",
            ));
        }
        Ok(())
    }

    pub async fn check_endpoint_before_deletion(
        &self,
        _conn: &DatabaseTransaction,
        _endpoint: &Endpoint,
    ) -> NotifyExchangeResult<()> {
        // Currently, nothing is needed to be checked
        Ok(())
    }
}

#[derive(Debug, Default, Eq, PartialEq)]
pub enum RefreshEncryptSecretOp {
    Create,
    Update,
    #[default]
    Any,
}

#[derive(Serialize)]
struct DispatchMessageRequest {
    topic_code: String,
    topic_user_id: Id,
    sub_id: Id,
    encrypted_message: String,
    offset: i64,
    nonce: String,
    encrypt_secret_code: String,
}

pub struct HttpDispatcherClient {
    context: Context,
    topic_code: String,
    topic_user_id: Id,
    subscription_id: Id,
    pub_keys: Vec<(EndpointSecret, Bytes)>,
    encrypt_secret: (EndpointSecret, Bytes),
    url: String,
    biz_code: String,
    client: Client,
}

impl HttpDispatcherClient {
    pub(super) async fn new(
        context: &Context,
        transport_service: &TransportService,
        endpoint: &Endpoint,
        subscription: &Subscription,
    ) -> NotifyExchangeResult<Option<Self>> {
        TransportServiceService::check_relations(
            transport_service,
            endpoint,
            subscription,
            TransportServiceType::Http,
        )?;

        let endpoint_options = HttpService::extract_endpoint_options(&endpoint.options)?;

        let topic = context
            .topic_service()
            .find_by_id(context.db(), subscription.topic_id)
            .await?;

        let biz_code = endpoint.id.to_string();
        let Some(url) = endpoint_options.url.filter(|u| !u.is_empty()) else {
            tracing::debug!("No webhook url set, endpoint[{}]", biz_code);
            return Ok(None);
        };
        let Some(encrypt_secret_code) = endpoint_options.esc_in_push else {
            tracing::debug!("No webhook encrypt secret code set, endpoint[{}]", biz_code);
            return Ok(None);
        };

        let endpoint_service = context.endpoint_service();
        let mut pub_keys = vec![];
        for pub_key in endpoint_service
            .decrypt_endpoint_secrets(
                context.db(),
                endpoint_service
                    .find_secrets_by_id(
                        context.db(),
                        endpoint.id,
                        HttpService::E_SECRET_TYPE_AUTH_PUB_KEY,
                        None,
                        Some(true),
                        false,
                    )
                    .await?,
            )
            .await?
        {
            let data = BASE64_STANDARD.decode(pub_key.1).map_err(|e| {
                tracing::warn!("RSA public key is not base64 encoded: {:?}", e);
                error::internal_server_error("RSA public key is not base64 encoded")
            })?;
            pub_keys.push((pub_key.0, data.into()));
        }
        if pub_keys.is_empty() {
            tracing::warn!(
                "There is no valid pub key for http endpoint[{}]",
                endpoint.id
            );
            return Ok(None);
        }
        let encrypt_secrets = endpoint_service
            .find_secrets_by_id(
                context.db(),
                endpoint.id,
                HttpService::E_SECRET_TYPE_ENCRYPT_SECRET,
                Some(&encrypt_secret_code),
                Some(true),
                false,
            )
            .await?;
        // use the latest one
        let Some(encrypt_secret) = endpoint_service
            .decrypt_endpoint_secrets(context.db(), encrypt_secrets)
            .await?
            .pop()
        else {
            tracing::warn!(
                "There is no valid encrypt secret for http endpoint[{}]",
                endpoint.id
            );
            return Ok(None);
        };

        let client = Client::builder()
            .timeout(Duration::from_secs(300))
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(60))
            .build()?;

        Ok(Some(Self {
            context: context.clone(),
            topic_code: topic.code,
            topic_user_id: topic.user_id,
            subscription_id: subscription.id,
            pub_keys,
            encrypt_secret,
            url,
            biz_code,
            client,
        }))
    }
}

impl DispatcherClient for HttpDispatcherClient {
    async fn dispatch<F>(
        &mut self,
        message: &Message,
        data: Vec<u8>,
        has_changed: F,
    ) -> Result<(), bool>
    where
        F: Fn() -> bool,
    {
        let encrypt_key_data = &self.encrypt_secret.1;
        let nonce = AesGcmOptions::new().nonce;

        let encrypted_message = match secret::encrypt_aes_gcm(&data, encrypt_key_data, &nonce) {
            Ok(data) => BASE64_STANDARD.encode(data),
            Err(e) => {
                // It shouldn't happen, so if it happens there is no need to retry.
                tracing::error!("Failed to encrypt message: {e:?}");
                return Err(true);
            }
        };
        let req = DispatchMessageRequest {
            topic_code: self.topic_code.clone(),
            topic_user_id: self.topic_user_id,
            sub_id: self.subscription_id,
            encrypted_message,
            offset: message.id,
            nonce: BASE64_STANDARD.encode(&nonce),
            encrypt_secret_code: self.encrypt_secret.0.code.clone(),
        };

        let mut count = 0;
        loop {
            let req = self.client.post(&self.url).json(&req);
            let mut off_by_error = false;
            match self.call(req).await {
                Ok(resp) => {
                    if resp.status() == StatusCode::OK {
                        return Ok(());
                    } else {
                        tracing::warn!(
                            "The endpoint returned a response with verified signature but not http 200"
                        );
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to send message to {}: {e:?}", self.url);
                    match e {
                        // Network or endpoint error, setting the subscription off
                        NotifyExchangeError::Reqwest { .. } => off_by_error = true,
                        NotifyExchangeError::InvalidResponse { .. } => off_by_error = true,
                        NotifyExchangeError::InvalidSign { .. } => off_by_error = true,
                        // System err, retry without setting the subscription off
                        _ => {}
                    }
                }
            }

            let timeout = Duration::from_secs((5 + 1) << count);
            count += 1;
            if has_changed() {
                return Err(false);
            }
            if count > 10 {
                return Err(off_by_error);
            }
            time::sleep(timeout).await
        }
    }
}

impl HttpApiCallerAuthHelper for HttpDispatcherClient {
    fn max_body_size_without_temp_file(&self) -> usize {
        self.context.config().max_body_size_without_temp_file()
    }

    fn temp_dir(&self) -> Option<&str> {
        self.context.config().temp_dir().as_deref()
    }

    async fn sign_algo(&self) -> NotifyExchangeResult<SignAlgo> {
        Ok(SignAlgo::Sha256Rsa)
    }

    async fn request_biz_code(&self) -> NotifyExchangeResult<String> {
        Ok(HttpService::SERVER_BIZ_CODE.to_string())
    }

    async fn response_biz_code(&self) -> NotifyExchangeResult<Option<String>> {
        Ok(Some(self.biz_code.to_string()))
    }

    async fn private_key(&self) -> NotifyExchangeResult<Zeroizing<Vec<u8>>> {
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

    async fn public_keys(&self, biz_code: &str) -> NotifyExchangeResult<Vec<&Bytes>> {
        if biz_code != self.biz_code {
            tracing::error!("Invalid biz code: {biz_code}, expected: {}", self.biz_code);
            return Err(error::invalid_response("Invalid biz code"));
        }
        Ok(self.pub_keys.iter().map(|(_, bs)| bs).collect())
    }
}

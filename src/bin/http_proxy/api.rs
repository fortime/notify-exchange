use base64::{Engine, prelude::BASE64_STANDARD};
use bytes::Bytes;
use notify_exchange::{
    auth::{HttpApiCallerAuthHelper, SignAlgo},
    db::custom_type::Id,
    error::{self, NotifyExchangeResult},
    service::secret::{self, AesGcmOptions},
    util,
};
use reqwest::{Client, RequestBuilder, StatusCode, Url};
use rsa::pkcs8::der::zeroize::Zeroizing;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::config::Account;

impl HttpApiCallerAuthHelper for Account {
    fn max_body_size_without_temp_file(&self) -> usize {
        self.max_body_size_without_temp_file()
            .unwrap_or(0x4000_0000)
    }

    fn temp_dir(&self) -> Option<&str> {
        self.temp_dir().as_deref()
    }

    async fn sign_algo(&self) -> NotifyExchangeResult<SignAlgo> {
        Ok(SignAlgo::Sha256Rsa)
    }

    async fn request_biz_code(&self) -> NotifyExchangeResult<String> {
        Ok(self.biz_code().clone())
    }

    async fn response_biz_code(&self) -> NotifyExchangeResult<Option<String>> {
        Ok(self.default_server_pub_key_code().map(ToString::to_string))
    }

    async fn private_key(&self) -> NotifyExchangeResult<Zeroizing<Vec<u8>>> {
        Ok(self.default_client_pri_key().1.clone())
    }

    async fn public_keys(&self, biz_code: &str) -> NotifyExchangeResult<Vec<&Bytes>> {
        let Some(pub_key) = self.server_pub_key(biz_code) else {
            tracing::error!("Unknown biz code {biz_code}");
            return Err(error::invalid_response("Unknown biz code"));
        };
        Ok(vec![pub_key])
    }
}

pub struct ApiClient<'a> {
    account: &'a Account,
    client: &'a Client,
}

#[derive(Deserialize)]
#[serde(bound = "T: DeserializeOwned")]
struct ApiResponse<T> {
    #[allow(unused)]
    status: String,
    data: T,
}

#[derive(Debug, Deserialize)]
struct ApiErrorResponse {
    #[allow(unused)]
    status: String,
    #[allow(unused)]
    message: String,
}

#[derive(Serialize)]
struct RefreshEncryptSecretRequest<'a> {
    code: &'a str,
    /// if force is false, no new secret will be created when the current version is created in the
    /// past one hour.
    force: bool,
}

#[derive(Serialize)]
struct PublishMessageRequest<'a> {
    topic_code: &'a str,
    topic_user_id: Option<Id>,
    encrypted_message: &'a str,
    nonce: &'a str,
    encrypt_secret_code: &'a str,
}

#[derive(Serialize)]
struct PullMessageQuery<'a> {
    topic_code: &'a str,
    topic_user_id: Option<Id>,
    limit: u8,
    poll_timeout: Option<u8>,
    encrypt_secret_code: &'a str,
}

#[derive(Debug, Deserialize)]
pub struct MessageAndOffset {
    pub data: String,
    pub offset: i64,
}

#[derive(Debug, Deserialize)]
pub struct PullMessageResponse {
    pub sub_id: Id,
    pub encrypted_messages: Vec<MessageAndOffset>,
    pub nonce: String,
}

#[derive(Serialize)]
struct CommitOffsetRequest {
    sub_id: Id,
    offset: i64,
}

#[derive(Deserialize)]
struct RefreshEncryptSecretResponse {
    /// Base64 encoded RSA encrypted secret, if no secret created, it will be null.
    encrypted_secret: Option<String>,
}

impl<'a> ApiClient<'a> {
    pub fn new(account: &'a Account, client: &'a Client) -> Self {
        Self { account, client }
    }

    fn url(&self, path: &str) -> NotifyExchangeResult<Url> {
        let mut url = self.account.base_url().clone();
        util::extend_url(&mut url, path)?;
        Ok(url)
    }

    async fn call<R>(&self, request_builder: RequestBuilder) -> NotifyExchangeResult<R>
    where
        R: DeserializeOwned,
    {
        let resp = match self.account.call(request_builder).await {
            Ok(resp) => resp,
            Err(e) => {
                tracing::error!("Calling api with error: {e:#?}");
                return Err(error::internal_server_error(
                    "Failed to call notify-exchange server",
                ));
            }
        };
        if resp.status() == StatusCode::OK {
            Ok(resp.json::<ApiResponse<R>>().await?.data)
        } else {
            match resp.json::<ApiErrorResponse>().await {
                Ok(e) => Err(error::invalid_response(format!("Error response: {e:?}"))),
                Err(e) => Err(error::invalid_response(format!("Invalid response: {e:?}"))),
            }
        }
    }

    pub async fn refresh_encrypt_secret(&self) -> NotifyExchangeResult<Bytes> {
        let resp: RefreshEncryptSecretResponse = self
            .call(self.client.post(self.url("endpoint/encrypt-secret")?).json(
                &RefreshEncryptSecretRequest {
                    code: self.account.encrypt_secret_code(),
                    force: true,
                },
            ))
            .await?;
        let encrypted_secret = resp
            .encrypted_secret
            .ok_or_else(|| error::invalid_response("No secret is returned"))?;
        let secret = BASE64_STANDARD
            .decode(encrypted_secret)
            .map_err(|_| error::internal_server_error("encrypted_secret is not base64 encoded"))?;
        secret::decrypt_rsa(&secret, self.account.default_client_pri_key().1).map(Into::into)
    }

    pub async fn publish_message(
        &self,
        topic_code: &str,
        topic_user_id: Option<Id>,
        message: &[u8],
    ) -> NotifyExchangeResult<()> {
        let secret = self.account.init_encrypt_secret(self.client).await?;
        let nonce = AesGcmOptions::new().nonce;

        let encrypted_message =
            BASE64_STANDARD.encode(secret::encrypt_aes_gcm(message, &secret, &nonce)?);

        let _resp: () = self
            .call(
                self.client
                    .post(self.url("message")?)
                    .json(&PublishMessageRequest {
                        topic_code,
                        topic_user_id,
                        encrypted_message: &encrypted_message,
                        nonce: &BASE64_STANDARD.encode(&nonce),
                        encrypt_secret_code: self.account.encrypt_secret_code(),
                    }),
            )
            .await?;
        Ok(())
    }

    pub async fn pull_message(
        &self,
        topic_code: &str,
        topic_user_id: Option<Id>,
        limit: u8,
        poll_timeout: Option<u8>,
    ) -> NotifyExchangeResult<(Id, Vec<(i64, Vec<u8>)>)> {
        let secret = self.account.init_encrypt_secret(self.client).await?;
        let resp: PullMessageResponse = self
            .call(
                self.client
                    .get(self.url("message")?)
                    .query(&PullMessageQuery {
                        topic_code,
                        topic_user_id,
                        limit,
                        poll_timeout,
                        encrypt_secret_code: self.account.encrypt_secret_code(),
                    }),
            )
            .await?;

        let nonce = BASE64_STANDARD
            .decode(&resp.nonce)
            .map_err(|_| error::invalid_response("nonce is not base64 encoded"))?;

        let mut decrypted_messages = Vec::with_capacity(resp.encrypted_messages.len());
        for msg in resp.encrypted_messages {
            let data = BASE64_STANDARD
                .decode(&msg.data)
                .map_err(|_| error::invalid_response("message data is not base64 encoded"))?;
            let decrypted_data = secret::decrypt_aes_gcm(&data, &secret, &nonce)?;
            decrypted_messages.push((msg.offset, decrypted_data));
        }

        Ok((resp.sub_id, decrypted_messages))
    }

    pub async fn commit_offset(&self, sub_id: Id, offset: i64) -> NotifyExchangeResult<()> {
        let _resp: () = self
            .call(
                self.client
                    .post(self.url("offset")?)
                    .json(&CommitOffsetRequest { sub_id, offset }),
            )
            .await?;
        Ok(())
    }
}

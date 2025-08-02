use std::{collections::HashMap, net::IpAddr, path::PathBuf, sync::Arc};

use base64::{Engine as _, prelude::BASE64_STANDARD_NO_PAD};
use bytes::Bytes;
use clap::Parser;
use figment::{
    Figment,
    providers::{Env, Format as _, Serialized, Toml},
};
use getset::{CopyGetters, Getters};
use notify_exchange::error::NotifyExchangeResult;
use reqwest::{Client, Url};
use rsa::{
    RsaPrivateKey, RsaPublicKey,
    pkcs8::{
        DecodePrivateKey as _, DecodePublicKey as _, EncodePrivateKey as _, EncodePublicKey as _,
        LineEnding, der::zeroize::Zeroizing,
    },
};
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{DeserializeOwned, Error as DeError},
    ser::Error as SerError,
};
use tokio::sync::RwLock;

use crate::api::ApiClient;

const ENV_PREFIX: &str = "NE_HTTP_PROXY";

#[derive(Serialize, Deserialize)]
#[serde(bound = "T: Serialize + DeserializeOwned")]
struct CodedValue<T> {
    code: String,
    value: T,
}

#[derive(Debug)]
struct CodedValueRef<'a> {
    #[allow(unused)]
    code: &'a str,
    #[allow(unused)]
    value: &'a str,
}

#[derive(Serialize, Deserialize)]
pub struct RawAccount {
    passwords: Vec<String>,
    base_url: String,
    biz_code: String,
    encrypt_secret_code: String,
    server_pub_keys: Vec<CodedValue<String>>,
    client_pri_keys: Vec<CodedValue<String>>,
    default_client_pri_key_code: String,
    temp_dir: Option<String>,
    max_body_size_without_temp_file: Option<usize>,
}

type PubKeyWithRaw = (RsaPublicKey, Arc<Bytes>);
type PriKeyWithRaw = (RsaPrivateKey, Arc<Zeroizing<Vec<u8>>>);

#[derive(Getters, CopyGetters)]
pub struct Account {
    passwords: Vec<String>,
    #[getset(get = "pub")]
    base_url: Url,
    #[getset(get = "pub")]
    biz_code: String,
    #[getset(get = "pub")]
    encrypt_secret_code: String,
    server_pub_keys: Vec<CodedValue<PubKeyWithRaw>>,
    client_pri_keys: Vec<CodedValue<PriKeyWithRaw>>,
    #[getset(get = "pub")]
    default_client_pri_key_code: String,
    #[getset(get = "pub")]
    temp_dir: Option<String>,
    #[getset(get_copy = "pub")]
    max_body_size_without_temp_file: Option<usize>,
    encrypt_secret_value: RwLock<Option<Arc<Bytes>>>,
    client: Client,
}

impl Serialize for Account {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut server_pub_keys = Vec::with_capacity(self.server_pub_keys.len());
        for pub_key in &self.server_pub_keys {
            let value = pub_key
                .value
                .0
                .to_public_key_pem(LineEnding::LF)
                .map_err(|e| {
                    SerError::custom(format!(
                        "Unable to generate pem string for server pub key[{}]: {e:?}",
                        pub_key.code
                    ))
                })?;
            server_pub_keys.push(CodedValue {
                code: pub_key.code.clone(),
                value: BASE64_STANDARD_NO_PAD.encode(value),
            });
        }
        let mut client_pri_keys = Vec::with_capacity(self.client_pri_keys.len());
        for pri_key in &self.client_pri_keys {
            let value = pri_key.value.0.to_pkcs8_pem(LineEnding::LF).map_err(|e| {
                SerError::custom(format!(
                    "Unable to generate pem string for client pri key[{}]: {e:?}",
                    pri_key.code
                ))
            })?;
            client_pri_keys.push(CodedValue {
                code: pri_key.code.clone(),
                value: BASE64_STANDARD_NO_PAD.encode(value),
            });
        }
        RawAccount {
            passwords: self.passwords.clone(),
            base_url: self.base_url.to_string(),
            biz_code: self.biz_code.clone(),
            encrypt_secret_code: self.encrypt_secret_code.clone(),
            server_pub_keys,
            client_pri_keys,
            default_client_pri_key_code: self.default_client_pri_key_code.clone(),
            temp_dir: self.temp_dir.clone(),
            max_body_size_without_temp_file: self.max_body_size_without_temp_file,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Account {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw_account = RawAccount::deserialize(deserializer)?;
        let mut server_pub_keys = Vec::with_capacity(raw_account.server_pub_keys.len());
        for pub_key in raw_account.server_pub_keys {
            let value = BASE64_STANDARD_NO_PAD
                .decode(&pub_key.value)
                .map_err(|e| {
                    DeError::custom(format!(
                        "Invalid base64 encoding of server pub key[{}]: {e:?}",
                        pub_key.code
                    ))
                })
                .and_then(|pem| {
                    String::from_utf8(pem).map_err(|_| {
                        DeError::custom(format!(
                            "Invalid string of server pub key[{}]",
                            pub_key.code
                        ))
                    })
                })
                .and_then(|pem| {
                    RsaPublicKey::from_public_key_pem(&pem)
                        .and_then(|pub_key| {
                            let bs = Bytes::from(pub_key.to_public_key_der()?.into_vec());
                            Ok((pub_key, Arc::new(bs)))
                        })
                        .map_err(|_| {
                            DeError::custom(format!(
                                "Invalid pem pub key of server pub key[{}]",
                                pub_key.code
                            ))
                        })
                })?;
            server_pub_keys.push(CodedValue {
                code: pub_key.code,
                value,
            });
        }
        let mut has_default = false;
        let mut client_pri_keys = Vec::with_capacity(raw_account.client_pri_keys.len());
        for pri_key in raw_account.client_pri_keys {
            if pri_key.code == raw_account.default_client_pri_key_code {
                has_default = true;
            }
            let value = BASE64_STANDARD_NO_PAD
                .decode(&pri_key.value)
                .map_err(|e| {
                    DeError::custom(format!(
                        "Invalid base64 encoding of client pri key[{}]: {e:?}",
                        pri_key.code
                    ))
                })
                .and_then(|pem| {
                    String::from_utf8(pem).map_err(|_| {
                        DeError::custom(format!(
                            "Invalid string of client pri key[{}]",
                            pri_key.code
                        ))
                    })
                })
                .and_then(|pem| {
                    RsaPrivateKey::from_pkcs8_pem(&pem)
                        .and_then(|pri_key| {
                            let bs = pri_key.to_pkcs8_der()?.to_bytes();
                            Ok((pri_key, Arc::new(bs)))
                        })
                        .map_err(|_| {
                            DeError::custom(format!(
                                "Invalid pem pri key of client pri key[{}]",
                                pri_key.code
                            ))
                        })
                })?;
            client_pri_keys.push(CodedValue {
                code: pri_key.code,
                value,
            });
        }
        let Ok(base_url) = raw_account.base_url.parse() else {
            return Err(DeError::custom(format!(
                "Invalid base url: {}",
                raw_account.base_url
            )));
        };

        if !has_default {
            Err(DeError::custom("No default client pri key found"))
        } else {
            Ok(Account {
                passwords: raw_account.passwords,
                base_url,
                biz_code: raw_account.biz_code,
                encrypt_secret_code: raw_account.encrypt_secret_code,
                server_pub_keys,
                client_pri_keys,
                default_client_pri_key_code: raw_account.default_client_pri_key_code,
                temp_dir: raw_account.temp_dir,
                max_body_size_without_temp_file: raw_account.max_body_size_without_temp_file,
                encrypt_secret_value: Default::default(),
                client: Default::default(),
            })
        }
    }
}

impl std::fmt::Debug for Account {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Account")
            .field("passwords", &format!("{} passwords", self.passwords.len()))
            .field(
                "server_pub_keys",
                &self
                    .server_pub_keys
                    .iter()
                    .map(|v| CodedValueRef {
                        code: &v.code,
                        value: "MASKED",
                    })
                    .collect::<Vec<_>>(),
            )
            .field(
                "client_pri_keys",
                &self
                    .client_pri_keys
                    .iter()
                    .map(|v| CodedValueRef {
                        code: &v.code,
                        value: "MASKED",
                    })
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl Account {
    pub fn as_api_client(&self) -> ApiClient<'_> {
        ApiClient::new(self, &self.client)
    }

    pub async fn init_encrypt_secret(&self, client: &Client) -> NotifyExchangeResult<Arc<Bytes>> {
        if let Some(secret) = self.encrypt_secret_value.read().await.as_ref() {
            return Ok(secret.clone());
        }
        let mut secret = self.encrypt_secret_value.write().await;

        if let Some(secret) = secret.as_ref() {
            return Ok(secret.clone());
        }

        let value = Arc::new(
            ApiClient::new(self, client)
                .refresh_encrypt_secret()
                .await?,
        );
        *secret = Some(value.clone());
        Ok(value)
    }

    #[allow(unused)]
    pub async fn refresh_encrypt_secret(
        &self,
        client: &Client,
    ) -> NotifyExchangeResult<Arc<Bytes>> {
        let mut secret = self.encrypt_secret_value.write().await;

        let value = Arc::new(
            ApiClient::new(self, client)
                .refresh_encrypt_secret()
                .await?,
        );
        *secret = Some(value.clone());
        Ok(value)
    }

    pub fn default_client_pri_key(&self) -> (&str, &Zeroizing<Vec<u8>>) {
        for pri_key in &self.client_pri_keys {
            if pri_key.code == self.default_client_pri_key_code {
                return (&pri_key.code, &pri_key.value.1);
            }
        }
        unreachable!(
            "Account can only be created via deserialize, and it should check the default key exists"
        )
    }

    #[allow(unused)]
    pub fn client_pri_key(&self, code: &str) -> Option<&Zeroizing<Vec<u8>>> {
        for pri_key in &self.client_pri_keys {
            if pri_key.code == code {
                return Some(&pri_key.value.1);
            }
        }
        None
    }

    pub fn default_server_pub_key_code(&self) -> Option<&str> {
        self.server_pub_keys.first().map(|v| v.code.as_str())
    }

    pub fn server_pub_key(&self, code: &str) -> Option<&Bytes> {
        for pub_key in &self.server_pub_keys {
            if pub_key.code == code {
                return Some(&pub_key.value.1);
            }
        }
        None
    }

    pub fn authenticate(&self, password: &str) -> bool {
        let idx = self.passwords.iter().position(|p| p == password);
        tracing::debug!("Password index: {:?}", idx);
        idx.is_some()
    }
}

#[derive(Debug, Parser, Serialize, Deserialize, Getters, CopyGetters)]
pub struct Config {
    /// The path of config file.
    #[arg(short = 'c', long = "config", value_name = "PATH")]
    path: Option<PathBuf>,

    /// The ip address to be bound to.
    #[getset(get_copy = "pub")]
    #[arg(long, value_name = "IP", default_value = "127.0.0.1")]
    bind_ip: IpAddr,

    /// The port to be bound to.
    #[getset(get_copy = "pub")]
    #[arg(long, value_name = "PORT", default_value_t = 8082)]
    bind_port: u16,

    #[getset(get_copy = "pub")]
    #[arg(long, default_missing_value = "true")]
    log_timestamp: bool,

    /// Accounts.
    #[getset(get = "pub")]
    #[serde(default)]
    #[clap(skip)]
    accounts: HashMap<String, Arc<Account>>,
}

impl Config {
    pub fn parse() -> NotifyExchangeResult<Self> {
        let config = <Config as Parser>::parse();
        config.fill()
    }

    fn fill(self) -> NotifyExchangeResult<Self> {
        let file_data = if let Some(path) = &self.path {
            Toml::file(path)
        } else {
            Toml::string("")
        };
        let config: Self = Figment::new()
            .merge(Serialized::defaults(self))
            .merge(file_data)
            .merge(Env::prefixed(ENV_PREFIX))
            .extract()?;
        Ok(config)
    }
}

use aes_gcm::{AeadCore, Aes256Gcm, KeyInit, Nonce, aead::Aead};
use base64::{
    Engine,
    prelude::{BASE64_STANDARD, BASE64_URL_SAFE},
};
use bytes::Bytes;
use chrono::Utc;
use futures::{FutureExt, future::BoxFuture};
use rsa::{
    Pkcs1v15Encrypt, RsaPrivateKey, RsaPublicKey,
    pkcs8::{self, DecodePrivateKey, DecodePublicKey, EncodePrivateKey, EncodePublicKey},
};
use sea_orm::{
    ActiveModelTrait as _, ColumnTrait as _, ConnectionTrait, EntityTrait as _,
    IntoActiveModel as _, QueryFilter as _,
};

use crate::{
    config::secret::{EncryptedKey, Key},
    context::Context,
    db::{
        custom_type::{EncryptionType, Id, KeySource},
        entity::{Secret, SecretColumn, SecretDsl},
    },
    encoding::lv::{Length, LvReader, LvWriter},
    error::{self, NotifyExchangeError, NotifyExchangeResult},
};

pub enum EncryptionOptions {
    Rsa,
    AesGcm(AesGcmOptions),
}

impl EncryptionOptions {
    pub fn aes_gcm() -> Self {
        EncryptionOptions::AesGcm(AesGcmOptions::new())
    }
}

impl TryFrom<EncryptionOptions> for Bytes {
    type Error = NotifyExchangeError;

    fn try_from(value: EncryptionOptions) -> Result<Self, Self::Error> {
        match value {
            EncryptionOptions::Rsa => Ok(Bytes::new()),
            EncryptionOptions::AesGcm(options) => options.try_into(),
        }
    }
}

pub struct AesGcmOptions {
    /// nonce
    pub nonce: Bytes,
}

impl AesGcmOptions {
    pub fn new() -> Self {
        let nonce = Aes256Gcm::generate_nonce(&mut rand::thread_rng()).to_vec();
        Self {
            nonce: nonce.into(),
        }
    }
}

impl Default for AesGcmOptions {
    fn default() -> Self {
        AesGcmOptions::new()
    }
}

impl From<AesGcmOptions> for EncryptionOptions {
    fn from(options: AesGcmOptions) -> Self {
        EncryptionOptions::AesGcm(options)
    }
}

impl TryFrom<Bytes> for AesGcmOptions {
    type Error = NotifyExchangeError;

    fn try_from(data: Bytes) -> Result<Self, Self::Error> {
        let mut reader: LvReader<u16> = LvReader::new(data);
        let nonce = read_field(&mut reader, "AesGcmOptions", "nonce")?;
        Ok(Self { nonce })
    }
}

impl TryFrom<AesGcmOptions> for Bytes {
    type Error = NotifyExchangeError;

    fn try_from(options: AesGcmOptions) -> Result<Self, Self::Error> {
        let mut writer: LvWriter<u16> = Default::default();
        write_field(&mut writer, "AesGcmOptions", "nonce", &options.nonce)?;
        Ok(writer.freeze())
    }
}

pub trait SecretLikeCreator<T> {
    fn create(
        self,
        encrypt_key_code: String,
        encrypt_key_source: KeySource,
        encryption_type: EncryptionType,
        options: String,
        encrypted_data: String,
    ) -> NotifyExchangeResult<T>;
}

impl<T, F> SecretLikeCreator<T> for F
where
    F: FnOnce(String, KeySource, EncryptionType, String, String) -> NotifyExchangeResult<T>,
{
    fn create(
        self,
        encrypt_key_code: String,
        encrypt_key_source: KeySource,
        encryption_type: EncryptionType,
        options: String,
        encrypted_data: String,
    ) -> NotifyExchangeResult<T> {
        self(
            encrypt_key_code,
            encrypt_key_source,
            encryption_type,
            options,
            encrypted_data,
        )
    }
}

pub trait SecretLike: Sized {
    fn encrypt_key_code(&self) -> &str;

    fn encrypt_key_source(&self) -> KeySource;

    fn encryption_type(&self) -> EncryptionType;

    fn options(&self) -> &str;

    fn encrypted_data(&self) -> &str;
}

pub struct SecretService<'a> {
    context: &'a Context,
}

impl<'a> SecretService<'a> {
    pub const MAIN_KEY_CODE: &'static str = "key-main";

    pub const MAIN_RSA_KEY_CODE: &'static str = "key-main-rsa";

    pub const AUTH_RSA_KEY_CODE: &'static str = "key-auth-rsa";

    async fn insert_encrypted_key<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        code: &str,
        encrypted_key: EncryptedKey,
        description: Option<String>,
    ) -> NotifyExchangeResult<Secret> {
        let now = Utc::now();
        let secret = match encrypted_key {
            EncryptedKey::Plain(key) => {
                // If the key is plain, we don't need to insert it into the database
                Secret {
                    id: Id::new(),
                    code: code.to_string(),
                    encryption_type: EncryptionType::Plain,
                    key_source: KeySource::Config,
                    key_code: "".to_string(),
                    data: key.to_string(),
                    options: BASE64_URL_SAFE.encode(Bytes::new()),
                    description,
                    created_at: now,
                    updated_at: now,
                }
            }
            EncryptedKey::Encrypted {
                encryption_type,
                code: encrypt_code,
                source: encrypt_source,
                options,
                data,
            } => Secret {
                id: Id::new(),
                code: code.to_string(),
                encryption_type,
                key_source: encrypt_source,
                key_code: encrypt_code,
                data: BASE64_URL_SAFE.encode(data),
                options: BASE64_URL_SAFE.encode(options),
                description,
                created_at: now,
                updated_at: now,
            },
        };
        let secret = secret.into_active_model().insert(conn).await?;

        Ok(secret)
    }

    async fn init_rsa_key<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        key_code: &str,
        description: &str,
    ) -> NotifyExchangeResult<()> {
        if self
            .find_key_by_code_from_db(conn, key_code)
            .await?
            .is_none()
        {
            let rsa_key = new_rsa_key().map_err(|e| {
                tracing::error!("Failed to generate RSA key: {:?}", e);
                error::internal_server_error("Failed to generate RSA key")
            })?;
            let encrypted_key = self
                .encrypt_key(
                    conn,
                    &rsa_key,
                    EncryptionType::SaltedAesGcm,
                    Self::MAIN_KEY_CODE,
                    KeySource::Config,
                    None,
                )
                .await?;
            self.insert_encrypted_key(conn, key_code, encrypted_key, Some(description.to_string()))
                .await?;
        }
        Ok(())
    }

    pub async fn init_rsa_keys<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
    ) -> NotifyExchangeResult<()> {
        for (key_code, description) in &[
            (Self::MAIN_RSA_KEY_CODE, "Main RSA key"),
            (Self::AUTH_RSA_KEY_CODE, "Auth RSA key"),
        ] {
            self.init_rsa_key(conn, key_code, description).await?;
        }
        Ok(())
    }

    pub async fn find_secret<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        id: Id,
    ) -> NotifyExchangeResult<Option<Vec<u8>>> {
        let secret = SecretDsl::find_by_id(id).one(conn).await?;

        if let Some(secret) = secret {
            self.decrypt_secret(conn, &secret).await.map(Some)
        } else {
            Ok(None)
        }
    }

    async fn find_key<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        code: &str,
        source: KeySource,
    ) -> NotifyExchangeResult<Option<Key>> {
        match source {
            KeySource::Config => {
                if let Some(encrypted_key) = self.context.config().get_key(code) {
                    Ok(Some(self.decrypt_key(conn, encrypted_key).await?))
                } else {
                    Ok(None)
                }
            }
            KeySource::Db => self.find_key_by_code_from_db(conn, code).boxed().await,
        }
    }

    pub async fn find_key_by_code_from_db<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        code: &str,
    ) -> NotifyExchangeResult<Option<Key>> {
        let secret = SecretDsl::find()
            .filter(SecretColumn::Code.eq(code))
            .one(conn)
            .await?;

        if let Some(secret) = secret {
            self.decrypt_key(conn, &EncryptedKey::try_from(secret)?)
                .await
                .map(Some)
        } else {
            Ok(None)
        }
    }

    pub async fn decrypt_secret<Conn, S>(
        &self,
        conn: &Conn,
        secret: &S,
    ) -> NotifyExchangeResult<Vec<u8>>
    where
        Conn: ConnectionTrait,
        S: SecretLike,
    {
        let key = self
            .find_key(conn, secret.encrypt_key_code(), secret.encrypt_key_source())
            .await?
            .ok_or_else(|| {
                error::internal_server_error(format!(
                    "Key[{:?}/{}] not found",
                    secret.encrypt_key_source(),
                    secret.encrypt_key_code()
                ))
            })?;
        let options = BASE64_URL_SAFE.decode(secret.options()).map_err(|e| {
            tracing::error!(
                "The field options is Not a valid base64 url string: {}, {:?}",
                secret.options(),
                e
            );
            error::internal_server_error("Not a valid base64 url string")
        })?;
        self.decrypt_base64_url_string(
            secret.encrypted_data(),
            secret.encryption_type(),
            &key,
            options.into(),
        )
    }

    fn decrypt_key<'b, Conn: ConnectionTrait>(
        &'b self,
        conn: &'b Conn,
        encrypted_key: &'b EncryptedKey,
    ) -> BoxFuture<'b, NotifyExchangeResult<Key>> {
        async move {
            match encrypted_key {
                EncryptedKey::Plain(decrypted_key) => Ok(decrypted_key.clone()),
                EncryptedKey::Encrypted {
                    encryption_type,
                    code,
                    source,
                    options,
                    data,
                } => {
                    let key = self.find_key(conn, code, *source).await?;
                    let Some(key) = key else {
                        tracing::error!("Key[{source:?}/{code}] not found");
                        return Err(error::internal_server_error("Key not found"));
                    };
                    let data = self.decrypt(data, *encryption_type, &key, options.clone())?;
                    Key::try_from(Bytes::from(data))
                }
            }
        }
        .boxed()
    }

    fn decrypt_base64_url_string(
        &self,
        s: &str,
        encryption_type: EncryptionType,
        key: &Key,
        options: Bytes,
    ) -> NotifyExchangeResult<Vec<u8>> {
        let data = BASE64_URL_SAFE.decode(s).map_err(|e| {
            tracing::error!("Not a valid base64 url string: {}, {:?}", s, e);
            error::internal_server_error("Not a valid base64 url string")
        })?;
        self.decrypt(&data, encryption_type, key, options)
    }

    fn decrypt<D>(
        &self,
        data: D,
        encryption_type: EncryptionType,
        key: &Key,
        options: Bytes,
    ) -> NotifyExchangeResult<Vec<u8>>
    where
        D: AsRef<[u8]>,
    {
        match encryption_type {
            EncryptionType::Plain => Ok(data.as_ref().to_vec()),
            EncryptionType::SaltedAesGcm => {
                let Key::AesGcm(key) = key else {
                    return Err(error::internal_server_error("Not a aes gcm key"));
                };
                let mut key = key.to_vec();
                let min_len = key.len().min(self.context.secret_salt().len());
                for (idx, k) in key.iter_mut().enumerate().take(min_len) {
                    *k ^= self.context.secret_salt()[idx];
                }
                let options = AesGcmOptions::try_from(options)?;
                decrypt_aes_gcm(data.as_ref(), &key, &options.nonce)
            }
            EncryptionType::Rsa => {
                if !options.is_empty() {
                    return Err(error::internal_server_error(
                        "Invalid rsa encryption options",
                    ));
                }
                let Key::Rsa {
                    pri_key,
                    pub_key: _,
                } = key
                else {
                    return Err(error::internal_server_error("Not a rsa key"));
                };

                decrypt_rsa_with_base64_key(data.as_ref(), pri_key)
            }
            EncryptionType::AesGcm => {
                let options = AesGcmOptions::try_from(options)?;
                let Key::AesGcm(key) = key else {
                    return Err(error::internal_server_error("Not a aes gcm key"));
                };
                decrypt_aes_gcm(data.as_ref(), key, &options.nonce)
            }
        }
    }

    async fn find_or_create_default_key<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
    ) -> NotifyExchangeResult<(String, Key)> {
        let key_code = Utc::now()
            .format(self.context.config().default_key_code_pattern())
            .to_string();
        if let Some(key) = self.find_key_by_code_from_db(conn, &key_code).await? {
            Ok((key_code, key))
        } else {
            let key = new_aes_gcm_key();
            let encrypted_key = self
                .encrypt_key(
                    conn,
                    &key,
                    EncryptionType::Rsa,
                    Self::MAIN_RSA_KEY_CODE,
                    KeySource::Db,
                    None,
                )
                .await?;
            self.insert_encrypted_key(
                conn,
                &key_code,
                encrypted_key,
                Some("Generated key".to_string()),
            )
            .await?;
            Ok((key_code, key))
        }
    }

    pub async fn insert_secret<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        code: &str,
        data: &[u8],
        encrypt_key: Option<(&str, KeySource, EncryptionType)>,
        description: Option<String>,
    ) -> NotifyExchangeResult<Secret> {
        let secret = self
            .encrypt_secret(
                conn,
                data,
                encrypt_key,
                move |encrypt_key_code,
                      encrypt_key_source,
                      encryption_type,
                      options,
                      encrypted_data| {
                    let now = Utc::now();
                    Ok(Secret {
                        id: Id::new(),
                        code: code.to_string(),
                        encryption_type,
                        key_source: encrypt_key_source,
                        key_code: encrypt_key_code,
                        data: encrypted_data,
                        options,
                        description,
                        created_at: now,
                        updated_at: now,
                    })
                },
            )
            .await?;

        let secret = secret.into_active_model().insert(conn).await?;

        Ok(secret)
    }

    pub async fn encrypt_secret<Conn: ConnectionTrait, C, T>(
        &self,
        conn: &Conn,
        data: &[u8],
        encrypt_key: Option<(&str, KeySource, EncryptionType)>,
        c: C,
    ) -> NotifyExchangeResult<T>
    where
        C: SecretLikeCreator<T>,
    {
        let (encrypt_key, encrypt_key_code, encrypt_key_source, encryption_type, options) =
            match encrypt_key {
                Some((code, source, encryption_type)) => {
                    let key = self
                        .find_key(conn, code, source)
                        .await?
                        .ok_or_else(|| error::internal_server_error("Key not found"))?;
                    let options = match encryption_type {
                        EncryptionType::Rsa | EncryptionType::Plain => EncryptionOptions::Rsa,
                        EncryptionType::AesGcm | EncryptionType::SaltedAesGcm => {
                            EncryptionOptions::aes_gcm()
                        }
                    };
                    (key, code.to_string(), source, encryption_type, options)
                }
                None => {
                    // Use default dynamic key
                    let (code, key) = self.find_or_create_default_key(conn).await?;
                    (
                        key,
                        code,
                        KeySource::Db,
                        EncryptionType::SaltedAesGcm,
                        EncryptionOptions::aes_gcm(),
                    )
                }
            };
        let encrypted_data =
            self.encrypt_base64_url_string(data, encryption_type, &encrypt_key, &options)?;
        let options = BASE64_URL_SAFE.encode(Bytes::try_from(options)?);

        c.create(
            encrypt_key_code,
            encrypt_key_source,
            encryption_type,
            options,
            encrypted_data,
        )
    }

    async fn encrypt_key<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        key: &Key,
        encryption_type: EncryptionType,
        encrypt_key_code: &str,
        encrypt_key_source: KeySource,
        encrypt_key: Option<Key>,
    ) -> NotifyExchangeResult<EncryptedKey> {
        let options = match encryption_type {
            EncryptionType::Rsa => EncryptionOptions::Rsa,
            EncryptionType::AesGcm | EncryptionType::SaltedAesGcm => EncryptionOptions::aes_gcm(),
            EncryptionType::Plain => {
                return Ok(EncryptedKey::Plain(key.clone()));
            }
        };

        let encrypt_key = match encrypt_key {
            Some(key) => key,
            None => {
                let Some(encrypt_key) = self
                    .find_key(conn, encrypt_key_code, encrypt_key_source)
                    .await?
                else {
                    tracing::error!("Key[{encrypt_key_source:?}/{encrypt_key_code}] not found");
                    return Err(error::internal_server_error("Key not found"));
                };
                encrypt_key
            }
        };

        let encrypted_data = self.encrypt(
            Vec::<u8>::from(key),
            encryption_type,
            &encrypt_key,
            &options,
        )?;
        Ok(EncryptedKey::Encrypted {
            encryption_type,
            code: encrypt_key_code.to_string(),
            source: encrypt_key_source,
            options: options.try_into()?,
            data: encrypted_data.into(),
        })
    }

    fn encrypt_base64_url_string<D>(
        &self,
        data: D,
        encryption_type: EncryptionType,
        key: &Key,
        options: &EncryptionOptions,
    ) -> NotifyExchangeResult<String>
    where
        D: AsRef<[u8]>,
    {
        let data = self.encrypt(&data, encryption_type, key, options)?;
        Ok(BASE64_URL_SAFE.encode(data))
    }

    fn encrypt<D>(
        &self,
        data: D,
        encryption_type: EncryptionType,
        key: &Key,
        options: &EncryptionOptions,
    ) -> NotifyExchangeResult<Vec<u8>>
    where
        D: AsRef<[u8]>,
    {
        match encryption_type {
            EncryptionType::Plain => Ok(data.as_ref().to_vec()),
            EncryptionType::SaltedAesGcm => {
                let Key::AesGcm(key) = key else {
                    return Err(error::internal_server_error("Not a aes gcm key"));
                };
                let mut key = key.to_vec();
                let min_len = key.len().min(self.context.secret_salt().len());
                for (idx, k) in key.iter_mut().enumerate().take(min_len) {
                    *k ^= self.context.secret_salt()[idx];
                }
                let EncryptionOptions::AesGcm(options) = options else {
                    return Err(error::internal_server_error(
                        "Expect AesGcmOptions, but not",
                    ));
                };
                encrypt_aes_gcm(data.as_ref(), &key, &options.nonce)
            }
            EncryptionType::Rsa => {
                let EncryptionOptions::Rsa = options else {
                    return Err(error::internal_server_error("Expect RsaOptions, but not"));
                };
                let Key::Rsa {
                    pri_key: _,
                    pub_key,
                } = key
                else {
                    return Err(error::internal_server_error("Not a rsa key"));
                };

                encrypt_rsa_with_base64_key(data.as_ref(), pub_key)
            }
            EncryptionType::AesGcm => {
                let EncryptionOptions::AesGcm(options) = options else {
                    return Err(error::internal_server_error(
                        "Expect AesGcmOptions, but not",
                    ));
                };
                let Key::AesGcm(key) = key else {
                    return Err(error::internal_server_error("Not a aes gcm key"));
                };
                encrypt_aes_gcm(data.as_ref(), key, &options.nonce)
            }
        }
    }
}

fn read_field<L>(
    reader: &mut LvReader<L>,
    struct_name: &str,
    field_name: &str,
) -> NotifyExchangeResult<Bytes>
where
    L: Length,
{
    match reader.next() {
        Some(Ok(data)) => Ok(data),
        Some(Err(pos)) => Err(error::internal_server_error(format!(
            "Parsing field[{}.{}] failed at pos[{}]",
            struct_name, field_name, pos
        ))),
        None => Err(error::internal_server_error(format!(
            "Parsing failed[{}.{}] failed, data is not enough",
            struct_name, field_name,
        ))),
    }
}

fn write_field<L, D>(
    writer: &mut LvWriter<L>,
    struct_name: &str,
    field_name: &str,
    data: D,
) -> NotifyExchangeResult<usize>
where
    L: Length,
    D: AsRef<[u8]>,
{
    let data = data.as_ref();
    let size = writer.write(data);
    if size == 0 {
        Err(error::internal_server_error(format!(
            "Invalid [{}.{}], it is too long: {}",
            struct_name,
            field_name,
            data.len()
        )))
    } else {
        Ok(size)
    }
}

pub fn decrypt_rsa(data: &[u8], pri_key: &[u8]) -> NotifyExchangeResult<Vec<u8>> {
    let rsa_private_key = RsaPrivateKey::from_pkcs8_der(pri_key).map_err(|e| {
        tracing::warn!("Failed to parse RSA private key: {:?}", e);
        error::internal_server_error("Not a valid rsa private key")
    })?;

    rsa_private_key.decrypt(Pkcs1v15Encrypt, data).map_err(|e| {
        tracing::error!("RSA decryption failed: {:?}", e);
        error::internal_server_error("RSA decryption failed")
    })
}

pub fn decrypt_rsa_with_base64_key(data: &[u8], pri_key: &[u8]) -> NotifyExchangeResult<Vec<u8>> {
    let pri_key = BASE64_STANDARD.decode(pri_key).map_err(|e| {
        tracing::warn!("RSA private key is not base64 encoded: {:?}", e);
        error::internal_server_error("RSA private key is not base64 encoded")
    })?;
    decrypt_rsa(data, &pri_key)
}

pub fn decrypt_aes_gcm(data: &[u8], key: &[u8], nonce: &[u8]) -> NotifyExchangeResult<Vec<u8>> {
    if key.len() != 32 {
        return Err(error::internal_server_error(
            "The length of aes gcm key should be 32 bytes",
        ));
    }

    if nonce.len() != 12 {
        return Err(error::internal_server_error(
            "The length of aes gcm nonce should be 12 bytes",
        ));
    }

    // Alternatively, the key can be transformed directly from a byte slice
    // (panicks on length mismatch):
    let key = aes_gcm::Key::<Aes256Gcm>::from_slice(key);

    let cipher = Aes256Gcm::new(key);
    // (panicks on length mismatch):
    let nonce = Nonce::from_slice(nonce);
    cipher.decrypt(nonce, data).map_err(|e| {
        tracing::error!("AesGcm decryption failed: {:?}", e);
        error::internal_server_error("AesGcm decryption failed")
    })
}

pub fn encrypt_rsa(data: &[u8], pub_key: &[u8]) -> NotifyExchangeResult<Vec<u8>> {
    let rsa_public_key = RsaPublicKey::from_public_key_der(pub_key).map_err(|e| {
        tracing::warn!("Failed to parse RSA public key: {:?}", e);
        error::internal_server_error("Not a valid rsa public key")
    })?;

    rsa_public_key
        .encrypt(&mut rand::thread_rng(), Pkcs1v15Encrypt, data)
        .map_err(|e| {
            tracing::error!("RSA encryption failed: {:?}", e);
            error::internal_server_error("RSA encryption failed")
        })
}

pub fn encrypt_rsa_with_base64_key(data: &[u8], pub_key: &[u8]) -> NotifyExchangeResult<Vec<u8>> {
    let pub_key = BASE64_STANDARD.decode(pub_key).map_err(|e| {
        tracing::warn!("RSA public key is not base64 encoded: {:?}", e);
        error::internal_server_error("RSA public key is not base64 encoded")
    })?;
    encrypt_rsa(data, &pub_key)
}

pub fn encrypt_aes_gcm(data: &[u8], key: &[u8], nonce: &[u8]) -> NotifyExchangeResult<Vec<u8>> {
    if key.len() != 32 {
        return Err(error::internal_server_error(
            "The length of aes gcm key should be 32 bytes",
        ));
    }

    if nonce.len() != 12 {
        return Err(error::internal_server_error(
            "The length of aes gcm nonce should be 12 bytes",
        ));
    }

    // Alternatively, the key can be transformed directly from a byte slice
    // (panicks on length mismatch):
    let key = aes_gcm::Key::<Aes256Gcm>::from_slice(key);

    let cipher = Aes256Gcm::new(key);
    // (panicks on length mismatch):
    let nonce = Nonce::from_slice(nonce);
    cipher.encrypt(nonce, data).map_err(|e| {
        tracing::error!("AesGcm encryption failed: {:?}", e);
        error::internal_server_error("AesGcm encryption failed")
    })
}

impl Context {
    pub fn secret_service(&self) -> SecretService<'_> {
        SecretService { context: self }
    }
}

pub fn new_rsa_key() -> Result<Key, rsa::Error> {
    let pri_key = RsaPrivateKey::new(&mut rand::thread_rng(), 2048)?;
    let pub_key = RsaPublicKey::from(&pri_key);
    let pri_key = pri_key.to_pkcs8_der()?;
    let pub_key = pub_key
        .to_public_key_der()
        .map_err(pkcs8::Error::PublicKey)?;
    Ok(Key::Rsa {
        pri_key: Bytes::from(BASE64_STANDARD.encode(pri_key.as_bytes())),
        pub_key: Bytes::from(BASE64_STANDARD.encode(pub_key.as_bytes())),
    })
}

pub fn new_aes_gcm_key() -> Key {
    let key_bytes = Aes256Gcm::generate_key(&mut rand::thread_rng());
    Key::AesGcm(Bytes::from(key_bytes.to_vec()))
}

pub fn new_aes_gcm_key_data() -> Bytes {
    let key_bytes = Aes256Gcm::generate_key(&mut rand::thread_rng());
    Bytes::from(key_bytes.to_vec())
}

pub fn pub_key_pem_to_der(pub_key: &[u8]) -> NotifyExchangeResult<Bytes> {
    RsaPublicKey::from_public_key_pem(
        str::from_utf8(pub_key)
            .map_err(|_| error::invalid_request("Public key in pem is not a valid string"))?,
    )
    .and_then(|k| k.to_public_key_der())
    .map_err(|_| error::invalid_request("Invalid public key in pem"))
    .map(|k| Bytes::from(k.as_bytes().to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test RSA encryption and decryption with random rsa keys
    #[test]
    fn test_rsa_encryption() {
        let data = b"hello world";
        let key = new_rsa_key().expect("failed to generate rsa key");
        let Key::Rsa { pri_key, pub_key } = key else {
            unreachable!("Not a rsa key");
        };

        let encrypted = encrypt_rsa_with_base64_key(data, &pub_key).expect("failed to encrypt");
        let decrypted =
            decrypt_rsa_with_base64_key(&encrypted, &pri_key).expect("failed to decrypt");
        assert!(decrypted == data);
    }

    /// Test AES GCM encryption and decryption with random aes gcm key_source
    #[test]
    fn test_aes_gcm_encryption() {
        let key = new_aes_gcm_key();
        let options = AesGcmOptions::new();

        let Key::AesGcm(key) = key else {
            unreachable!("Not a aes gcm key");
        };

        let data = b"hello world";
        let encrypted = encrypt_aes_gcm(data, &key, &options.nonce).expect("failed to encrypt");
        let decrypted =
            decrypt_aes_gcm(&encrypted, &key, &options.nonce).expect("failed to decrypt");
        assert!(decrypted == data);
    }
}

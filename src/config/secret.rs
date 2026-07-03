use std::{
    fmt::{Display, Formatter, Result as FmtResult},
    str::FromStr,
};

use base64::{DecodeError, Engine as _, engine::general_purpose::URL_SAFE};
use bytes::Bytes;
use serde_with::{DeserializeFromStr, SerializeDisplay};

use crate::{
    db::{
        custom_type::{EncryptionType, KeySource, TryFromPrimitiveStr},
        entity::Secret,
    },
    error::{self, NotifyExchangeError, NotifyExchangeResult, NotifyExchangeResultExt},
};

/// EncryptedKey
///
/// a encrypted key can be serialize to/deserialize from a string like:
///
/// encrypted_type:code(Optional):source(Optional):options(Optional)|data(base64url string)
///
/// Why `DeserializeFromStr`, why not `<&str as Deserialize>::deserialize`?
///
/// StrVisitor doesn't override visit_str, and `figment` uses it.
#[derive(Debug, Clone, DeserializeFromStr, SerializeDisplay)]
pub enum EncryptedKey {
    Plain(Key),
    Encrypted {
        encryption_type: EncryptionType,
        /// the code of the key which is used to encrypted this key
        code: String,
        /// the source of the key which is used to encrypted this key
        source: KeySource,
        /// the options which is used to encrypted this key
        options: Bytes,
        data: Bytes,
    },
}

impl FromStr for EncryptedKey {
    type Err = NotifyExchangeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let items: Vec<_> = s.split(":").collect();
        if items.len() != 5 {
            return Err(error::invalid_key("Invalid format of encrypted key"));
        }
        let encryption_type = EncryptionType::try_from_primitive_str(items[0])?;

        let encrypted_key = match encryption_type {
            EncryptionType::Plain => {
                if !(items[1].is_empty() && items[2].is_empty() && items[3].is_empty()) {
                    return Err(error::invalid_key(
                        "Invalid value of plain encrypted key: code/source/options should be empty",
                    ));
                }
                EncryptedKey::Plain(items[4].parse()?)
            }
            _ => {
                let source = KeySource::try_from_primitive_str(items[2])?;
                let options = Bytes::from(base64_url_decode(items[3], |e| {
                    format!("Invalid base64 encoding of encrypted key options: {}", e)
                })?);
                let data = Bytes::from(base64_url_decode(items[4], |e| {
                    format!("Invalid base64 encoding of encrypted key data: {}", e)
                })?);
                EncryptedKey::Encrypted {
                    encryption_type,
                    code: items[1].to_string(),
                    source,
                    options,
                    data,
                }
            }
        };
        Ok(encrypted_key)
    }
}

impl Display for EncryptedKey {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            EncryptedKey::Plain(key) => {
                f.write_fmt(format_args!("{}::::{}", EncryptionType::Plain as i16, key,))
            }
            EncryptedKey::Encrypted {
                encryption_type,
                code,
                source,
                options,
                data,
            } => f.write_fmt(format_args!(
                "{}:{}:{}:{}:{}",
                *encryption_type as i16,
                code,
                *source as i16,
                URL_SAFE.encode(options),
                URL_SAFE.encode(data)
            )),
        }
    }
}

impl TryFrom<Secret> for EncryptedKey {
    type Error = NotifyExchangeError;

    fn try_from(secret: Secret) -> Result<Self, Self::Error> {
        let encrypted_key = match secret.encryption_type {
            EncryptionType::Plain => EncryptedKey::Plain(secret.data.parse()?),
            _ => EncryptedKey::Encrypted {
                encryption_type: secret.encryption_type,
                code: secret.key_code,
                source: secret.key_source,
                options: URL_SAFE
                    .decode(secret.options)
                    .whatever("Invalid base64 encoding of secret options")?
                    .into(),
                data: URL_SAFE
                    .decode(secret.data)
                    .whatever("Invalid base64 encoding of secret data")?
                    .into(),
            },
        };
        Ok(encrypted_key)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum Key {
    Rsa { pri_key: Bytes, pub_key: Bytes },
    AesGcm(Bytes),
}

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let ty = match self {
            Key::Rsa { .. } => "Rsa",
            Key::AesGcm(_) => "AesGcm",
        };
        f.write_fmt(format_args!("Key for {}", ty))
    }
}

impl FromStr for Key {
    type Err = NotifyExchangeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let data = Bytes::from(base64_url_decode(s, |e| {
            format!("Invalid base64 encoding of a key: {}", e)
        })?);
        Key::try_from(data)
    }
}

impl Display for Key {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.write_str(&URL_SAFE.encode(Vec::<u8>::from(self)))
    }
}

impl TryFrom<Bytes> for Key {
    type Error = NotifyExchangeError;

    fn try_from(mut data: Bytes) -> Result<Self, Self::Error> {
        let Some(pos) = data.iter().position(|b| *b == b':') else {
            return Err(error::invalid_key("Invalid format of key"));
        };
        let schema = data.split_to(pos + 1);
        let key = match &schema[..] {
            b"rsa:" => {
                let Some(pos) = data.iter().position(|b| *b == b':') else {
                    return Err(error::invalid_key("Invalid format of rsa key"));
                };
                let pri_key = data.split_to(pos);
                let pub_key = data.split_off(1);
                Key::Rsa { pri_key, pub_key }
            }
            b"aes_gcm:" => Key::AesGcm(data),
            _ => return Err(error::invalid_key("Unknown schema of key")),
        };
        Ok(key)
    }
}

impl From<&Key> for Vec<u8> {
    fn from(key: &Key) -> Self {
        let mut data = vec![];
        match key {
            Key::Rsa { pri_key, pub_key } => {
                data.extend_from_slice(b"rsa:");
                data.extend_from_slice(pri_key);
                data.extend_from_slice(b":");
                data.extend_from_slice(pub_key);
            }
            Key::AesGcm(bytes) => {
                data.extend_from_slice(b"aes_gcm:");
                data.extend_from_slice(bytes);
            }
        }
        data
    }
}

fn base64_url_decode<D, F>(d: D, f: F) -> NotifyExchangeResult<Vec<u8>>
where
    D: AsRef<[u8]>,
    F: FnOnce(DecodeError) -> String,
{
    URL_SAFE.decode(d).map_err(|e| error::invalid_key(f(e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test() -> NotifyExchangeResult<()> {
        // plain:rsa
        let s = "1::::cnNhOmNISnBkbUYwWlY5clpYaz06Y0hWaWJHbGpYMnRsZVE9PQ==";
        let encrypted_key = s.parse::<EncryptedKey>()?;

        assert!(s == encrypted_key.to_string());

        let EncryptedKey::Plain(key) = encrypted_key else {
            unreachable!("Not a plain encrypted key");
        };
        assert!(
            key == Key::Rsa {
                pri_key: Bytes::copy_from_slice(b"cHJpdmF0ZV9rZXk="),
                pub_key: Bytes::copy_from_slice(b"cHVibGljX2tleQ=="),
            }
        );

        // plain:aes_gcm
        let s = "1::::YWVzX2djbToxMjM0NTY=";
        let encrypted_key = s.parse::<EncryptedKey>()?;

        assert!(s == encrypted_key.to_string());

        let EncryptedKey::Plain(key) = encrypted_key else {
            unreachable!("Not a plain encrypted key");
        };
        assert!(key == Key::AesGcm(Bytes::copy_from_slice(b"123456")));

        // encrypted
        let s = "2:MAIN:2:bm9uY2U9MTIz:bm90IGVuY3J5cHRlZA=="; // spellchecker:disable-line
        let encrypted_key = s.parse::<EncryptedKey>()?;

        assert!(s == encrypted_key.to_string());

        let EncryptedKey::Encrypted {
            encryption_type,
            code,
            source,
            options,
            data,
        } = encrypted_key
        else {
            unreachable!("Not a plain encrypted key");
        };
        assert!(encryption_type == EncryptionType::SaltedAesGcm);
        assert!(code == "MAIN");
        assert!(source == KeySource::Db);
        assert!(*options == *b"nonce=123");
        assert!(*data == *b"not encrypted");

        Ok(())
    }
}

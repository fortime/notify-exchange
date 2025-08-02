use std::{collections::HashMap, net::IpAddr, path::PathBuf};

use chrono::NaiveTime;
use clap::{FromArgMatches, Parser, Subcommand};
use figment::{
    Figment,
    providers::{Env, Format, Serialized, Toml},
};
use getset::{CopyGetters, Getters};
use reqwest::Url;
use serde::{Deserialize, Serialize};

use crate::{config::secret::EncryptedKey, error::NotifyExchangeResult};

pub mod secret;

pub const ENV_PREFIX: &str = "NOTIFY_EXCHANGE_";

#[derive(Debug, Parser, Serialize, Deserialize)]
pub struct ConfigWithCommand<SC>
where
    SC: FromArgMatches + Subcommand,
{
    #[command(flatten)]
    pub config: Config,

    #[command(subcommand)]
    pub command: SC,
}

impl<SC> ConfigWithCommand<SC>
where
    SC: FromArgMatches + Subcommand,
{
    pub fn parse() -> NotifyExchangeResult<Self> {
        let Self { config, command } = <Self as Parser>::parse();

        Ok(Self {
            config: config.fill()?,
            command,
        })
    }
}

#[derive(Debug, Parser, Serialize, Deserialize, Getters, CopyGetters)]
pub struct Config {
    /// The path of config file.
    #[arg(short = 'c', long = "config", value_name = "PATH")]
    path: Option<PathBuf>,

    /// The path of secret salt file.
    #[getset(get = "pub")]
    #[arg(long, value_name = "PATH")]
    secret_salt_path: Option<PathBuf>,

    /// The public base url of the Notify Exchange server.
    #[getset(get = "pub")]
    #[arg(long, value_name = "URL", default_value = "http://127.0.0.1")]
    base_url: Url,

    /// The public base api url of the Notify Exchange server.
    #[getset(get = "pub")]
    #[arg(long, value_name = "URL", default_value = "http://127.0.0.1/api")]
    base_api_url: Url,

    /// The ip address to be bound to.
    #[getset(get_copy = "pub")]
    #[arg(long, value_name = "IP", default_value = "127.0.0.1")]
    bind_ip: IpAddr,

    /// The port to be bound to.
    #[getset(get_copy = "pub")]
    #[arg(long, value_name = "PORT", default_value_t = 8080)]
    bind_port: u16,

    /// The db url of the Notify Exchange server.
    #[getset(get = "pub")]
    #[arg(long, value_name = "URL", default_value = "")]
    db_url: String,

    /// The message db url of the Notify Exchange server, it's used to save messages. It shouldn't
    /// be the same as `db_url` and `cache_db_url` if you are using sqlite3.
    #[getset(get = "pub")]
    #[arg(long, value_name = "URL", default_value = "")]
    msg_db_url: String,

    /// The cache db url of the Notify Exchange server. It shouldn't be the same as `db_url` and
    /// `msg_db_url` if you are using sqlite3.
    #[getset(get = "pub")]
    #[arg(long, value_name = "URL", default_value = "")]
    cache_db_url: String,

    #[getset(get_copy = "pub")]
    #[arg(long, default_missing_value = "true")]
    log_timestamp: bool,

    /// We don't pass keys by arguments
    #[getset(get = "pub")]
    #[serde(default)]
    #[clap(skip)]
    key_store: HashMap<String, EncryptedKey>,

    /// The default key code pattern for generating key codes, it will be used by `chrono::DateTime.format`.
    #[getset(get = "pub")]
    #[arg(long, default_value = "default-key-%Y%m%d")]
    #[serde(default)]
    default_key_code_pattern: String,

    /// Maximum body size in bytes without using a temp file in a http api authentication.
    #[getset(get_copy = "pub")]
    #[arg(long, default_value_t = 0x4000_0000)]
    max_body_size_without_temp_file: usize,

    /// Temp directory for temp files. Default: `std::env::temp_dir`.
    #[getset(get = "pub")]
    #[arg(long)]
    temp_dir: Option<String>,

    /// Url path prefix in the signature calculation.
    #[getset(get = "pub")]
    #[arg(long)]
    http_auth_url_path_prefix: Option<String>,

    /// Refreshing dispatchers at.
    #[getset(get = "pub")]
    #[serde(default)]
    #[arg(long, default_value = "00:00:00")]
    refreshing_dispatchers_at: NaiveTime,

    /// Paths of web.
    #[getset(get = "pub")]
    #[serde(default)]
    #[clap(skip)]
    web_paths: WebPaths,
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

    pub fn get_key(&self, code: &str) -> Option<&EncryptedKey> {
        self.key_store().get(code)
    }
}

#[derive(Debug, Serialize, Deserialize, Getters)]
pub struct WebPaths {
    /// Web path for telegram login
    #[getset(get = "pub")]
    login_telegram_path: String,

    /// Web path for managing endpoint
    #[getset(get = "pub")]
    endpoint_path: String,
}

impl Default for WebPaths {
    fn default() -> Self {
        Self {
            login_telegram_path: "login/telegram".to_string(),
            endpoint_path: "endpoint/{endpoint_id}".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use crate::error;

    use super::*;

    fn default_config() -> Config {
        <Config as Parser>::parse_from(Vec::<OsString>::new())
    }

    #[test]
    fn test_serde_config() -> NotifyExchangeResult<()> {
        let mut config = default_config();
        config
            .key_store
            .insert("tmp-key".to_string(), "1::::YWVzX2djbToxMjM0NTY=".parse()?);

        let s = serde_json::to_string(&config)
            .map_err(error::serde_json_error_cb("unable to serialize config"))?;

        let config: Config = serde_json::from_str(&s)
            .map_err(error::serde_json_error_cb("unable to deserialize config"))?;
        assert!(config.key_store.contains_key("tmp-key"));
        Ok(())
    }

    #[test]
    fn test_figment_key_store() -> NotifyExchangeResult<()> {
        let config = default_config();
        let file_data = Toml::string(r#"key_store = {tmp-key="1::::YWVzX2djbToxMjM0NTY="}"#);
        let config: Config = Figment::new()
            .merge(Serialized::defaults(config))
            .merge(file_data)
            .extract()?;
        assert!(config.key_store.contains_key("tmp-key"));
        Ok(())
    }
}

#[cfg(feature = "console-subscriber")]
use std::fmt::{Display, Formatter, Result as FmtResult};

#[cfg(feature = "console-subscriber")]
use tempfile::TempPath;
use tempfile::{Builder, NamedTempFile};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::error::{NotifyExchangeResult, NotifyExchangeResultExt as _};

pub mod auth;
pub mod cache;
pub mod config;
pub mod context;
pub mod controller;
pub mod db;
pub mod dispatcher;
pub mod encoding;
pub mod error;
pub mod model;
pub mod service;
pub mod util;

pub struct LogGuard {
    #[cfg(feature = "console-subscriber")]
    socket: TempPath,
}

impl LogGuard {
    pub fn new() -> NotifyExchangeResult<Self> {
        Ok(Self {
            #[cfg(feature = "console-subscriber")]
            socket: NamedTempFile::with_prefix("ne-tokio-console-")
                .whatever("Unable create temp socket file for tokio-console")?
                .into_temp_path(),
        })
    }
}

#[cfg(feature = "console-subscriber")]
impl Display for LogGuard {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.write_fmt(format_args!("console_subscriber addr: {:?}", self.socket))?;
        Ok(())
    }
}

pub fn init_log(log_timestamp: bool) -> NotifyExchangeResult<LogGuard> {
    let subscriber = tracing_subscriber::registry().with(EnvFilter::from_default_env());
    let log_guard = LogGuard::new()?;
    #[cfg(feature = "console-subscriber")]
    let subscriber = {
        // use a tempfile as addr of console_subscriber
        let socket_file_path = log_guard.socket.to_path_buf();
        // delete the file, so console_subscriber can create it.
        std::fs::remove_file(&socket_file_path).whatever("Unable to delete temp socket file")?;
        subscriber.with(
            console_subscriber::ConsoleLayer::builder()
                .with_default_env()
                .server_addr(socket_file_path)
                .spawn(),
        )
    };

    if log_timestamp {
        subscriber.with(fmt::layer()).try_init()
    } else {
        subscriber.with(fmt::layer().without_time()).try_init()
    }
    .whatever("failed to init log")?;
    #[cfg(feature = "console-subscriber")]
    tracing::debug!("{}", log_guard);
    Ok(log_guard)
}

fn temp_file_with_prefix(
    temp_dir: Option<&str>,
    prefix: &str,
) -> NotifyExchangeResult<NamedTempFile> {
    let mut builder = Builder::new();
    builder.prefix(prefix);
    let file = if let Some(temp_dir) = temp_dir {
        if !std::fs::exists(temp_dir).whatever("Unable to check temp dir")? {
            std::fs::create_dir_all(temp_dir).whatever("Unable to create temp dir")?;
        }
        builder.tempfile_in(temp_dir)
    } else {
        builder.tempfile()
    };
    file.whatever("Unable to create temp file")
}

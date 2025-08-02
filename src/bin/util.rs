use base64::{Engine as _, prelude::BASE64_STANDARD};
use clap::Subcommand;
use notify_exchange::{
    config::{
        Config, ConfigWithCommand, ENV_PREFIX,
        secret::{EncryptedKey, Key},
    },
    context::{Context, SharedContext},
    dispatcher::DispatcherManager,
    error::{self, NotifyExchangeError, NotifyExchangeResult, NotifyExchangeResultExt as _},
    service::secret::{self, SecretService},
};
use rsa::{
    RsaPublicKey,
    pkcs8::{DecodePublicKey as _, EncodePublicKey as _, LineEnding},
};
use sea_orm::Database;
use serde::{Deserialize, Serialize};

#[derive(Debug, Subcommand, Serialize, Deserialize)]
enum Commands {
    /// Create a main key and print it as a config item
    CreateMainKey,
    PrintPubKeyInPem {
        code: String,
    },
}

fn create_main_key() {
    let key = secret::new_aes_gcm_key();
    let encrypted_key = EncryptedKey::Plain(key);

    println!(
        "Add environment variable to set the main key: \n{}KEY_STORE={{{}={}}}",
        ENV_PREFIX,
        SecretService::MAIN_KEY_CODE,
        encrypted_key
    );
}

async fn print_pub_key_in_pem(config: Config, code: &str) -> NotifyExchangeResult<()> {
    let context = context(config).await?;
    let key = match context
        .secret_service()
        .find_key_by_code_from_db(context.db(), code)
        .await
    {
        Ok(Some(key)) => key,
        Ok(None) => {
            tracing::error!("Key not found");
            return Ok(());
        }
        Err(e) => match e {
            NotifyExchangeError::InvalidKey { .. } => {
                tracing::error!("Not a valid key");
                return Ok(());
            }
            _ => {
                return Err(e);
            }
        },
    };
    match key {
        Key::Rsa { pub_key, .. } => {
            let pub_key = BASE64_STANDARD.decode(pub_key).map_err(|e| {
                tracing::error!("RSA private key is not base64 encoded: {:?}", e);
                error::internal_server_error("RSA private key is not base64 encoded")
            })?;
            let pub_key = RsaPublicKey::from_public_key_der(&pub_key)
                .whatever("Invalid public key format")?;
            match pub_key.to_public_key_pem(LineEnding::LF) {
                Ok(s) => println!("{s}"),
                Err(_) => tracing::warn!("Unable to write the public key in pem"),
            }
        }
        _ => {
            tracing::error!("Not a rsa key");
        }
    }

    Ok(())
}

async fn context(config: Config) -> NotifyExchangeResult<Context> {
    let database = Database::connect(config.db_url()).await?;
    let msg_database = Database::connect(config.msg_db_url()).await?;
    let cache_database = Database::connect(config.cache_db_url()).await?;
    let (dispatcher_manager, _dispatcher_manager_ev) = DispatcherManager::new();
    let shared_context = SharedContext::new(
        config,
        database.clone(),
        msg_database,
        dispatcher_manager,
        cache_database.into(),
    )?;
    Ok(Context::new(shared_context))
}

async fn run() -> NotifyExchangeResult<()> {
    let ConfigWithCommand { config, command } = ConfigWithCommand::<Commands>::parse()?;

    notify_exchange::init_log(config.log_timestamp())?;

    match command {
        Commands::CreateMainKey => create_main_key(),
        Commands::PrintPubKeyInPem { code } => print_pub_key_in_pem(config, &code).await?,
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    #[cfg(feature = "dotenv")]
    dotenv::dotenv().ok();

    if let Err(e) = run().await {
        eprintln!("failed to notify exchange util: {:#?}", e);
    }
}

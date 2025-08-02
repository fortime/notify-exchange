use std::time::Duration;

use axum::{Router, middleware, routing};
use chrono::{Days, Local, NaiveDateTime};
use sea_orm::Database;

use notify_exchange::{
    config::Config,
    context::{self, Context, SharedContext},
    controller::{self, admin::setup, webhook},
    dispatcher::DispatcherManager,
    error::{NotifyExchangeResult, NotifyExchangeResultExt},
    util::Signals,
};
use tokio::{net::TcpListener, signal::unix::SignalKind};
use tower_http::trace::TraceLayer;

// Wait for signals and do cleaning
fn graceful_shutdown(
    context: Context,
) -> NotifyExchangeResult<impl Future<Output = ()> + Send + 'static> {
    let signals = Signals::new(vec![
        SignalKind::terminate(),
        SignalKind::interrupt(),
        SignalKind::hangup(),
        SignalKind::pipe(),
        SignalKind::quit(),
    ])?;
    Ok(async move {
        tracing::info!("Waiting for signals: {:?}", signals);
        signals.await;
        if let Err(e) = clean(context).await {
            tracing::error!("Error occurred in cleaning: {e:?}")
        }
    })
}

async fn clean(context: Context) -> NotifyExchangeResult<()> {
    let conn = context.db();
    let transport_service_service = context.transport_service_service();
    let transport_services = transport_service_service.find_all_enabled(conn).await?;

    let operator = context.transport_service_operator();

    for transport_service in transport_services {
        operator.stop(conn, &transport_service).await?;
    }
    Ok(())
}

async fn run() -> NotifyExchangeResult<()> {
    let config = Config::parse()?;

    let _guard = notify_exchange::init_log(config.log_timestamp())?;

    // By default, sqlite3 will have only one connection, if we use the same db and we use cache in
    // a transaction, timeout will happen.
    let database = Database::connect(config.db_url()).await?;
    let msg_database = Database::connect(config.msg_db_url()).await?;
    let cache_database = Database::connect(config.cache_db_url()).await?;
    let (dispatcher_manager, dispatcher_manager_ev) = DispatcherManager::new();
    let shared_context = SharedContext::new(
        config,
        database.clone(),
        msg_database,
        dispatcher_manager,
        cache_database.into(),
    )?;
    let context = Context::new(shared_context);
    // Initialize the message id
    context.reset_next_message_id().await?;

    let transport_service_service = context.transport_service_service();
    let secret_service = context.secret_service();

    secret_service.init_rsa_keys(context.db()).await?;

    let mut app = Router::new().with_state(context.clone());

    let transport_services = transport_service_service
        .find_all_enabled(context.db())
        .await?;
    let listener = TcpListener::bind((context.config().bind_ip(), context.config().bind_port()))
        .await
        .whatever("Failed to listen to the address")?;
    tracing::info!(
        "Listen to {}:{}",
        context.config().bind_ip(),
        context.config().bind_port()
    );
    app = if transport_services.is_empty() {
        setup::route(app)
    } else {
        // Start all services
        let operator = context.transport_service_operator();
        for transport_service in transport_services {
            operator.start(context.db(), &transport_service).await?;
        }
        // Spawn a new task for running the dispatcher manager
        tokio::spawn({
            let context = context.clone();
            async move {
                let refreshing_at = context.config().refreshing_dispatchers_at();
                let naive_now = Local::now().naive_local();
                let mut naive_next = NaiveDateTime::new(naive_now.date(), *refreshing_at);
                if naive_next < naive_now {
                    naive_next = naive_next
                        .checked_add_days(Days::new(1))
                        .expect("Current datetime is too far away from this code was written");
                }
                let delay = naive_next
                    .signed_duration_since(naive_now)
                    .to_std()
                    .unwrap_or_default();
                context
                    .dispatcher_manager()
                    .run(delay, Duration::from_secs(24 * 3600))
                    .await
            }
        });
        app
    };

    app = app.route("/v1/initialized", routing::get(controller::initialized));
    app = webhook::telegram::route(app);
    app = webhook::http::route(app, &context);
    app = controller::user::route(app);
    app = controller::endpoint::route(app);
    app = controller::topic::route(app);
    app = controller::admin::pending_user::route(app);
    app = controller::admin::topic::route(app);
    app = controller::admin::transport_service::route(app);
    app = controller::admin::user::route(app);
    app = controller::admin::endpoint::route(app);

    app = app
        .layer(TraceLayer::new_for_http())
        .layer(middleware::from_fn_with_state(
            context.clone(),
            context::request_context_middleware,
        ));

    context.spawn_run(async |context| dispatcher_manager_ev.run(context).await);

    axum::serve(listener, app)
        // Stop all services after receiving the stop signal
        .with_graceful_shutdown(graceful_shutdown(context.clone())?)
        .await
        .whatever("Failed to start server")?;

    Ok(())
}

#[tokio::main]
async fn main() {
    #[cfg(feature = "dotenv")]
    dotenv::dotenv().ok();

    if let Err(e) = run().await {
        eprintln!("failed to start notify exchange: {:#?}", e);
    }
}

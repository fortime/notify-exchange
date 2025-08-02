use std::{collections::HashMap, sync::Arc, time::Duration};

use sea_orm::{
    ActiveModelTrait as _, ActiveValue, ColumnTrait as _, ConnectionTrait, EntityTrait as _,
    PaginatorTrait as _, QueryFilter as _, QueryOrder as _, QuerySelect as _,
};
use tokio::{
    sync::{
        mpsc::{
            self, UnboundedReceiver as MpscUnboundedReceiver,
            UnboundedSender as MpscUnboundedSender,
        },
        watch::{self, Receiver as WatchReceiver, Sender as WatchSender},
    },
    time,
};

use crate::{
    context::Context,
    db::{
        custom_type::{Id, SubscriptionStatus, TransportServiceType},
        entity::{
            ActiveSubscription, Endpoint, EndpointColumn, EndpointDsl, Message, MessageColumn,
            MessageDsl, Subscription, SubscriptionColumn, SubscriptionDsl, SubscriptionOffset,
            TransportService, TransportServiceDsl,
        },
    },
    error::{self, NotifyExchangeResult},
    service::{topic::SubscriptionService, transport::TransportServiceOperator},
};

const LIMIT: u64 = 20;

#[derive(Debug, Clone, Copy)]
enum DispatcherManagerEvent {
    RefreshAll,
    Load(Id),
    RefreshByTransportService(Id),
    RefreshByEndpoint(Id),
    Unload(Id),
}

pub struct DispatcherManagerEv {
    receiver: MpscUnboundedReceiver<DispatcherManagerEvent>,
}

impl DispatcherManagerEv {
    pub async fn run(mut self, context: Context) {
        let mut inner = DispatcherManagerInner::new(context);
        loop {
            let Some(event) = self.receiver.recv().await else {
                tracing::info!("Eventloop of DispatcherManager exits");
                break;
            };
            if let Err(e) = inner.handle_event(event).await {
                tracing::error!("Handle {event:?} failed: {e:#?}")
            }
        }
    }
}

struct DispatcherManagerInner {
    context: Context,
    senders: HashMap<Id, WatchSender<DispatcherOptions>>,
}

impl DispatcherManagerInner {
    fn new(context: Context) -> Self {
        Self {
            context,
            senders: Default::default(),
        }
    }

    async fn load_subscription(
        &mut self,
        transport_service: Arc<TransportService>,
        endpoint: Arc<Endpoint>,
        subscription: Subscription,
    ) -> NotifyExchangeResult<()> {
        if !transport_service.enabled {
            self.senders.remove(&subscription.id);
            tracing::info!("Remove the dispatcher of subscription[{}]", subscription.id);
            return Ok(());
        }
        let subscription_id = subscription.id;
        let options = DispatcherOptions {
            transport_service,
            endpoint,
            subscription,
        };
        let options = if let Some(sender) = self.senders.get(&subscription_id) {
            if let Err(e) = sender.send(options) {
                tracing::debug!(
                    "Receiver of Subscription[{}] has gone, recreate it",
                    subscription_id
                );
                e.0
            } else {
                // Successfully sent the new options to the existing refresher.
                return Ok(());
            }
        } else {
            options
        };

        let transport_service_type = options.transport_service.transport_service_type;
        let (sender, receiver) = watch::channel(options);
        self.senders.insert(subscription_id, sender);
        tracing::info!("Create the dispatcher of subscription[{}]", subscription_id);

        let context = self.context.clone();
        tokio::spawn(async move {
            let res = match transport_service_type {
                TransportServiceType::Telegram => {
                    let builder = async |context: &Context, options: &DispatcherOptions| {
                        TransportServiceOperator::build_telegram_dispatcher_client(
                            context,
                            &options.transport_service,
                            &options.endpoint,
                            &options.subscription,
                        )
                        .await
                    };
                    MessageDispatcher::run(context, receiver, builder).await
                }
                TransportServiceType::Http => {
                    let builder = async |context: &Context, options: &DispatcherOptions| {
                        TransportServiceOperator::build_http_dispatcher_client(
                            context,
                            &options.transport_service,
                            &options.endpoint,
                            &options.subscription,
                        )
                        .await
                    };
                    MessageDispatcher::run(context, receiver, builder).await
                }
            };
            if let Err(e) = res {
                tracing::error!(
                    "Dispatcher of subscription[{subscription_id}] exit with error: {e:#?}"
                );
            } else {
                tracing::info!("Dispatcher of subscription[{subscription_id}] exit");
            }
        });
        Ok(())
    }

    async fn refresh_all(&mut self) -> NotifyExchangeResult<()> {
        let context = self.context.clone();
        let mut transport_service_pages = TransportServiceDsl::find().paginate(context.db(), LIMIT);

        while let Some(transport_services) = transport_service_pages.fetch_and_next().await? {
            for transport_service in transport_services {
                let transport_service = Arc::new(transport_service);

                let mut endpoint_pages = EndpointDsl::find()
                    .filter(EndpointColumn::TransportServiceId.eq(transport_service.id))
                    .paginate(context.db(), LIMIT);
                while let Some(endpoints) = endpoint_pages.fetch_and_next().await? {
                    for endpoint in endpoints {
                        self.refresh_by_endpoint(transport_service.clone(), endpoint)
                            .await?;
                    }
                }
            }
        }

        Ok(())
    }

    async fn load(&mut self, subscription_id: Id) -> NotifyExchangeResult<()> {
        let Some((subscription, endpoint)) = SubscriptionDsl::find_by_id(subscription_id)
            .find_also_related(EndpointDsl)
            .one(self.context.db())
            .await?
        else {
            tracing::warn!("Subscription[{subscription_id}] not exist");
            return Ok(());
        };

        let Some(endpoint) = endpoint else {
            tracing::error!("Endpoint of subscription[{subscription_id}] not exist");
            return Ok(());
        };

        let Some(transport_service) =
            TransportServiceDsl::find_by_id(endpoint.transport_service_id)
                .one(self.context.db())
                .await?
        else {
            tracing::error!("TransportService of Endpoint[{}] not exist", endpoint.id);
            return Ok(());
        };

        self.load_subscription(
            Arc::new(transport_service),
            Arc::new(endpoint),
            subscription,
        )
        .await
    }

    async fn refresh_by_endpoint(
        &mut self,
        transport_service: Arc<TransportService>,
        endpoint: Endpoint,
    ) -> NotifyExchangeResult<()> {
        let context = self.context.clone();
        let endpoint = Arc::new(endpoint);

        let mut subscription_pages = SubscriptionDsl::find()
            .filter(SubscriptionColumn::EndpointId.eq(endpoint.id))
            .paginate(context.db(), LIMIT);

        while let Some(subscriptions) = subscription_pages.fetch_and_next().await? {
            for subscription in subscriptions {
                match subscription.status {
                    SubscriptionStatus::On => {
                        self.load_subscription(
                            transport_service.clone(),
                            endpoint.clone(),
                            subscription,
                        )
                        .await?;
                    }
                    _ => {
                        self.senders.remove(&subscription.id);
                        tracing::info!(
                            "Remove the dispatcher of subscription[{}]",
                            subscription.id
                        );
                    }
                }
            }
        }
        Ok(())
    }

    async fn refresh_by_transport_service_id(
        &mut self,
        transport_service_id: Id,
    ) -> NotifyExchangeResult<()> {
        let context = self.context.clone();
        // Make sure there is no writing of endpoint.
        let Some(transport_service) = TransportServiceDsl::find_by_id(transport_service_id)
            .one(self.context.db())
            .await?
        else {
            tracing::error!("TransportService not exist");
            return Ok(());
        };
        let transport_service = Arc::new(transport_service);

        let mut endpoint_pages = EndpointDsl::find()
            .filter(EndpointColumn::TransportServiceId.eq(transport_service.id))
            .paginate(context.db(), LIMIT);
        while let Some(endpoints) = endpoint_pages.fetch_and_next().await? {
            for endpoint in endpoints {
                self.refresh_by_endpoint(transport_service.clone(), endpoint)
                    .await?;
            }
        }
        Ok(())
    }

    async fn refresh_by_endpoint_id(&mut self, endpoint_id: Id) -> NotifyExchangeResult<()> {
        let context = self.context.clone();
        // Make sure there is no writing of endpoint.
        let Some(endpoint) = self
            .context
            .transaction(async |conn| {
                context
                    .endpoint_service()
                    .lock_endpoint(conn, endpoint_id, false)
                    .await
            })
            .await?
        else {
            tracing::warn!("Endpoint{endpoint_id} not exist");
            return Ok(());
        };
        let Some(transport_service) =
            TransportServiceDsl::find_by_id(endpoint.transport_service_id)
                .one(self.context.db())
                .await?
        else {
            tracing::error!("TransportService of Endpoint{endpoint_id} not exist");
            return Ok(());
        };
        self.refresh_by_endpoint(Arc::new(transport_service), endpoint)
            .await
    }

    async fn handle_event(&mut self, event: DispatcherManagerEvent) -> NotifyExchangeResult<()> {
        match event {
            DispatcherManagerEvent::RefreshAll => self.refresh_all().await,
            DispatcherManagerEvent::Load(subscription_id) => self.load(subscription_id).await,
            DispatcherManagerEvent::RefreshByTransportService(transport_service_id) => {
                self.refresh_by_transport_service_id(transport_service_id)
                    .await
            }
            DispatcherManagerEvent::RefreshByEndpoint(endpoint_id) => {
                self.refresh_by_endpoint_id(endpoint_id).await
            }
            DispatcherManagerEvent::Unload(subscription_id) => {
                self.senders.remove(&subscription_id);
                tracing::info!("Remove the dispatcher of subscription[{}]", subscription_id);
                Ok(())
            }
        }
    }
}

pub struct DispatcherManager {
    sender: MpscUnboundedSender<DispatcherManagerEvent>,
}

impl DispatcherManager {
    pub fn new() -> (Self, DispatcherManagerEv) {
        let (sender, receiver) = mpsc::unbounded_channel();
        (Self { sender }, DispatcherManagerEv { receiver })
    }

    fn send(&self, event: DispatcherManagerEvent) {
        if self.sender.send(event).is_err() {
            tracing::error!(
                "Eventloop of DispatcherManager has gone, event[{:?}] is skipped",
                event
            );
        }
    }

    pub fn refresh_all(&self) {
        self.send(DispatcherManagerEvent::RefreshAll)
    }

    pub fn load(&self, subscription_id: Id) {
        self.send(DispatcherManagerEvent::Load(subscription_id))
    }

    pub fn refresh_by_transport_service(&self, transport_service_id: Id) {
        self.send(DispatcherManagerEvent::RefreshByTransportService(
            transport_service_id,
        ))
    }

    pub fn refresh_by_endpoint(&self, endpoint_id: Id) {
        self.send(DispatcherManagerEvent::RefreshByEndpoint(endpoint_id))
    }

    pub fn unload(&self, subscription_id: Id) {
        self.send(DispatcherManagerEvent::Unload(subscription_id))
    }

    /// # Arguments
    ///
    /// * `first_delay`: delay before the first refresh, if it is not greater than 300 seconds, the
    ///   first refresh will be skipped.
    pub async fn run(&self, first_delay: Duration, interval: Duration) {
        self.refresh_all();

        time::sleep(first_delay).await;
        if first_delay.as_secs() > 300 {
            self.refresh_all();
        }

        let mut interval = time::interval(interval);
        loop {
            interval.tick().await;
            self.refresh_all();
        }
    }
}

#[derive(Clone)]
struct DispatcherOptions {
    transport_service: Arc<TransportService>,
    endpoint: Arc<Endpoint>,
    subscription: Subscription,
}

pub trait DispatcherClient {
    /// # Returns
    ///
    /// * Ok(()): the message has been dispatched.
    /// * Err(true): the message has failed to be dispatched, mark the subscription to
    ///   `OffByError`.
    /// * Err(false): the message has failed to be dispatched, don't mark the subscription to
    ///   `OffByError`, just exit the dispatcher.
    fn dispatch<F>(
        &mut self,
        message: &Message,
        data: Vec<u8>,
        has_changed: F,
    ) -> impl Future<Output = Result<(), bool>>
    where
        F: Fn() -> bool;
}

struct MessageDispatcher<B> {
    context: Context,
    receiver: WatchReceiver<DispatcherOptions>,
    builder: B,
}

impl<B, C> MessageDispatcher<B>
where
    B: AsyncFn(&Context, &DispatcherOptions) -> NotifyExchangeResult<Option<C>>,
    C: DispatcherClient + 'static,
{
    async fn create_client(&self, options: &DispatcherOptions) -> NotifyExchangeResult<Option<C>> {
        (self.builder)(&self.context, options).await
    }

    async fn dispatch(
        &self,
        client: &mut C,
        offset: &mut SubscriptionOffset,
        options: &DispatcherOptions,
    ) -> NotifyExchangeResult<()> {
        async fn commit<Conn: ConnectionTrait>(
            conn: &Conn,
            subscription_service: &SubscriptionService<'_>,
            offset: &mut SubscriptionOffset,
            options: &DispatcherOptions,
            latest_offset: Option<i64>,
        ) -> NotifyExchangeResult<()> {
            if let Some(latest_offset) = latest_offset {
                *offset = subscription_service
                    .commit(
                        conn,
                        &options.endpoint,
                        options.subscription.id,
                        latest_offset,
                    )
                    .await?;
            }
            Ok(())
        }

        let mut error = None;
        let mut latest_offset;
        let subscription_service = self.context.subscription_service();
        let secret_service = self.context.secret_service();
        'outer: loop {
            latest_offset = None;
            let messages = MessageDsl::find()
                .filter(MessageColumn::Id.gte(offset.next_message_id))
                .filter(MessageColumn::TopicId.eq(options.subscription.topic_id))
                .order_by_asc(MessageColumn::Id)
                .limit(LIMIT)
                .all(self.context.msg_db())
                .await;
            let messages = match messages {
                Ok(messages) => messages,
                Err(e) => {
                    error = Some(e.into());
                    break;
                }
            };
            if messages.is_empty() {
                break;
            }
            for message in messages {
                // If sender has dropped or we should update the client, we no longer send message
                if self.receiver.has_changed().unwrap_or(true) {
                    break 'outer;
                }
                // Send decrypted message
                if let Err(mark_error) = client
                    .dispatch(
                        &message,
                        secret_service
                            .decrypt_secret(self.context.db(), &message)
                            .await?,
                        || self.receiver.has_changed().unwrap_or(true),
                    )
                    .await
                {
                    if mark_error {
                        ActiveSubscription {
                            id: ActiveValue::Unchanged(options.subscription.id),
                            status: ActiveValue::Set(SubscriptionStatus::OffByError),
                            ..Default::default()
                        }
                        .update(self.context.db())
                        .await?;
                    }
                    error = Some(error::internal_server_error(
                        "Client failed to send message",
                    ));
                    break 'outer;
                };
                latest_offset = Some(message.id);
            }
            commit(
                self.context.db(),
                &subscription_service,
                offset,
                options,
                latest_offset,
            )
            .await?;
        }
        commit(
            self.context.db(),
            &subscription_service,
            offset,
            options,
            latest_offset,
        )
        .await?;
        if let Some(e) = error { Err(e) } else { Ok(()) }
    }

    pub async fn run(
        context: Context,
        receiver: WatchReceiver<DispatcherOptions>,
        builder: B,
    ) -> NotifyExchangeResult<()> {
        let mut dispatcher = MessageDispatcher {
            context,
            receiver,
            builder,
        };
        let mut options = dispatcher.receiver.borrow_and_update().clone();
        let topic_id = options.subscription.topic_id;
        let subscription_id = options.subscription.id;
        let Some(mut client) = dispatcher.create_client(&options).await? else {
            tracing::debug!(
                "There is no need to create a dispatcher for subscription[{subscription_id}]"
            );
            return Ok(());
        };
        let mut offset = dispatcher
            .context
            .subscription_service()
            .find_offset(dispatcher.context.db(), subscription_id)
            .await?;

        loop {
            tokio::select! {
                signal = dispatcher.context.incoming_message_signals().wait_new_message(topic_id) => {
                    if signal.is_err() {
                        tracing::info!("Topic[{topic_id}] is removed, exit");
                        break;
                    } else {
                        // Dispatch new messages
                        dispatcher.dispatch(&mut client, &mut offset, &options).await?;
                    }
                }
                changed = dispatcher.receiver.changed() => {
                    if changed.is_err() {
                        tracing::info!("Sender of Subscription[{subscription_id}] is gone, exit");
                        break;
                    } else {
                        options = dispatcher.receiver.borrow_and_update().clone();
                        if let Some(c) = dispatcher.create_client(&options).await? {
                            client = c;
                            offset = dispatcher.context.subscription_service().find_offset(dispatcher.context.db(), subscription_id).await?;
                        } else {
                            tracing::debug!("There is no need to create a dispatcher for subscription[{subscription_id}]");
                            return Ok(());
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

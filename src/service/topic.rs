use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    time::Duration,
};

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait as _, ActiveValue, ColumnTrait as _, ConnectionTrait, DatabaseTransaction,
    EntityTrait as _, IntoActiveModel as _, Iterable as _, Order, PaginatorTrait, QueryFilter as _,
    QueryOrder, QuerySelect as _, QueryTrait,
    prelude::Expr,
    sea_query::{Cond, SimpleExpr},
};
use tokio::sync::{
    Mutex,
    watch::{self, Receiver as WatchReceiver, Sender as WatchSender},
};
use validator::Validate as _;

use crate::{
    context::Context,
    db::{
        custom_type::{Id, SubscriptionStatus, TopicPermissionType},
        entity::{
            ActiveSubscriptionOffset, Endpoint, FirstPage, Message, MessageColumn, MessageDsl,
            Subscription, SubscriptionColumn, SubscriptionDsl, SubscriptionOffset,
            SubscriptionOffsetColumn, SubscriptionOffsetDsl, Topic, TopicColumn, TopicDsl,
            TopicPermission, TopicPermissionColumn, TopicPermissionDsl, User, UserColumn, UserDsl,
            UserTopic, UserTopicColumn, UserTopicDsl,
        },
    },
    error::{self, NotifyExchangeResult},
    model::req::CreateTopicRequest,
};

pub struct TopicService<'a> {
    context: &'a Context,
}

impl<'a> TopicService<'a> {
    pub async fn create_topic(
        &self,
        conn: &DatabaseTransaction,
        user_id: Id,
        req: CreateTopicRequest<'_>,
    ) -> NotifyExchangeResult<Topic> {
        req.validate()?;
        let CreateTopicRequest {
            name,
            code,
            description,
        } = req;
        // 1. Authorization: Only admins can create topics.
        if !self.context.user_service().is_admin(conn, user_id).await? {
            return Err(error::forbidden("Only admins can create topics"));
        }

        // 2. Validation: Check for duplicate topic (code, user_id).
        if TopicDsl::find()
            .filter(TopicColumn::Code.eq(code.as_ref()))
            .filter(TopicColumn::UserId.eq(user_id))
            .one(conn)
            .await?
            .is_some()
        {
            return Err(error::conflict("Topic with this code already exists"));
        }

        // 3. Insert Topic
        let now = Utc::now();
        let new_topic = Topic {
            id: Id::new(),
            code: code.into_owned(),
            name: name.into_owned(),
            user_id,
            description: description.map(Cow::into_owned),
            created_at: now,
            updated_at: now,
        };

        let topic = new_topic.into_active_model().insert(conn).await?;
        UserTopic {
            id: Id::new(),
            topic_id: topic.id,
            user_id,
            created_at: now,
            updated_at: now,
        }
        .into_active_model()
        .insert(conn)
        .await?;

        // 4. Grant all permissions to the creator.
        let permissions = TopicPermissionType::iter()
            .map(|permission_type| {
                TopicPermission {
                    id: Id::new(),
                    topic_id: topic.id,
                    user_id,
                    permission_type,
                    created_at: now,
                    updated_at: now,
                }
                .into_active_model()
            })
            .collect::<Vec<_>>();

        TopicPermissionDsl::insert_many(permissions)
            .exec(conn)
            .await?;

        Ok(topic)
    }

    pub async fn delete_topic(
        &self,
        conn: &DatabaseTransaction,
        user_id: Id,
        topic_id: Id,
    ) -> NotifyExchangeResult<u64> {
        // 1. Authorization: Only admins can delete topics.
        if !self.context.user_service().is_admin(conn, user_id).await? {
            return Err(error::forbidden("Only admins can create topics"));
        }

        // Ensure topic exists before trying to delete its children
        let topic = TopicDsl::find_by_id(topic_id)
            .one(conn)
            .await?
            .ok_or_else(|| error::not_found("Topic not found"))?;

        // 2. Delete related subscriptions
        self.context
            .subscription_service()
            .delete_by_topic_id(conn, topic.id)
            .await?;

        // 3. Delete related permissions
        TopicPermissionDsl::delete_many()
            .filter(TopicPermissionColumn::TopicId.eq(topic.id))
            .exec(conn)
            .await?;

        // 4. Delete user topics
        UserTopicDsl::delete_many()
            .filter(UserTopicColumn::TopicId.eq(topic.id))
            .exec(conn)
            .await?;

        // 5. Delete the topic itself
        let res = TopicDsl::delete_by_id(topic.id).exec(conn).await?;

        // 6. Tell others the topic is deleted
        self.context
            .incoming_message_signals()
            .remove_topic(topic.id)
            .await;

        Ok(res.rows_affected)
    }

    pub async fn subscribe(
        &self,
        conn: &DatabaseTransaction,
        topic_id: Id,
        endpoint: &Endpoint,
        subscriber_id: Id,
    ) -> NotifyExchangeResult<Subscription> {
        self.context
            .transport_service_operator()
            .check_endpoint_subscribable(conn, endpoint)
            .await?;

        let permissions = if endpoint.is_public {
            &[TopicPermissionType::ReadInPublic]
        } else {
            &[TopicPermissionType::Read]
        };

        // Check if the user of the endpoint has the permission to read the topic
        if !self
            .has_permissions(conn, topic_id, subscriber_id, permissions)
            .await?
        {
            return Err(error::forbidden("No permission to subscribe to the topic"));
        }

        let next_message_id = self
            .find_latest_message_id_by_topic(topic_id)
            .await?
            .map(|i| i + 1)
            .unwrap_or(0);

        self.context
            .subscription_service()
            .insert(conn, topic_id, endpoint.id, subscriber_id, next_message_id)
            .await
    }

    pub async fn subscribe_by_uniq_key(
        &self,
        conn: &DatabaseTransaction,
        topic_code: &str,
        topic_user_id: Option<Id>,
        endpoint: &Endpoint,
        subscriber_id: Id,
    ) -> NotifyExchangeResult<Subscription> {
        let topic_user_id = topic_user_id.unwrap_or(subscriber_id);
        let topic = self
            .find_by_uniq_key(conn, topic_code, topic_user_id)
            .await?;

        self.subscribe(conn, topic.id, endpoint, subscriber_id)
            .await
    }

    pub async fn unsubscribe(
        &self,
        conn: &DatabaseTransaction,
        topic_id: Id,
        endpoint: &Endpoint,
    ) -> NotifyExchangeResult<()> {
        let subscription = SubscriptionDsl::find()
            .filter(SubscriptionColumn::TopicId.eq(topic_id))
            .filter(SubscriptionColumn::EndpointId.eq(endpoint.id))
            .one(conn)
            .await?;
        if let Some(subscription) = subscription {
            self.context
                .subscription_service()
                .delete(conn, subscription.id)
                .await?;
        }
        Ok(())
    }

    pub async fn ubsubscribe_by_uniq_key(
        &self,
        conn: &DatabaseTransaction,
        topic_code: &str,
        topic_user_id: Option<Id>,
        endpoint: &Endpoint,
        subscriber_id: Id,
    ) -> NotifyExchangeResult<()> {
        let topic_user_id = topic_user_id.unwrap_or(subscriber_id);
        let topic = self
            .find_by_uniq_key(conn, topic_code, topic_user_id)
            .await?;

        self.unsubscribe(conn, topic.id, endpoint).await
    }

    pub async fn publish<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        topic_id: Id,
        endpoint_id: Id,
        publisher_id: Id,
        message: &[u8],
    ) -> NotifyExchangeResult<i64> {
        let permissions = &[TopicPermissionType::Write];

        // Check if the user of the endpoint has the permission to read the topic
        if !self
            .has_permissions(conn, topic_id, publisher_id, permissions)
            .await?
        {
            return Err(error::forbidden("No permission to write to the topic"));
        }

        let message_id = self.context.next_message_id().await?;
        let message = self
            .context
            .secret_service()
            .encrypt_secret(
                conn,
                message,
                None,
                move |encrypt_key_code,
                      encrypt_key_source,
                      encryption_type,
                      options,
                      encrypted_data| {
                    let now = Utc::now();
                    Ok(Message {
                        id: message_id,
                        encryption_type,
                        key_source: encrypt_key_source,
                        key_code: encrypt_key_code,
                        options,
                        topic_id,
                        data: encrypted_data,
                        user_id: publisher_id,
                        endpoint_id,
                        created_at: now,
                    })
                },
            )
            .await?;

        message
            .into_active_model()
            .insert(self.context.msg_db())
            .await?;

        // Notify others
        self.context
            .incoming_message_signals()
            .new_message(topic_id, message_id)
            .await;
        Ok(message_id)
    }

    pub async fn publish_by_uniq_key<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        topic_code: &str,
        topic_user_id: Option<Id>,
        endpoint_id: Id,
        publisher_id: Id,
        message: &[u8],
    ) -> NotifyExchangeResult<i64> {
        let topic_user_id = topic_user_id.unwrap_or(publisher_id);
        let topic = self
            .find_by_uniq_key(conn, topic_code, topic_user_id)
            .await?;

        self.publish(conn, topic.id, endpoint_id, publisher_id, message)
            .await
    }

    pub async fn pull(
        &self,
        topic_id: Id,
        offset: i64,
        limit: usize,
        timeout: Duration,
    ) -> NotifyExchangeResult<Vec<Message>> {
        let mut waited = false;
        loop {
            let messages = MessageDsl::find()
                .filter(MessageColumn::TopicId.eq(topic_id))
                .filter(MessageColumn::Id.gte(offset))
                .limit(Some(limit as u64))
                .order_by_asc(MessageColumn::Id)
                .all(self.context.msg_db())
                .await?;
            if messages.is_empty() && !waited {
                if let Some(Ok(_)) = self
                    .context
                    .incoming_message_signals()
                    .wait_new_message_with_timeout(topic_id, timeout)
                    .await
                {
                    waited = true;
                    continue;
                }
                return Ok(messages);
            }
            return Ok(messages);
        }
    }

    pub async fn find_by_id<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        topic_id: Id,
    ) -> NotifyExchangeResult<Topic> {
        TopicDsl::find_by_id(topic_id)
            .one(conn)
            .await?
            .ok_or_else(|| error::not_found("Topic not exist"))
    }

    pub async fn find_by_uniq_key<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        code: &str,
        user_id: Id,
    ) -> NotifyExchangeResult<Topic> {
        let topic = TopicDsl::find()
            .filter(TopicColumn::Code.eq(code))
            .filter(TopicColumn::UserId.eq(user_id))
            .one(conn)
            .await?;
        let Some(topic) = topic else {
            return Err(error::not_found("Topic not exist"));
        };
        Ok(topic)
    }

    pub async fn has_any_permissions<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        topic_id: Id,
        user_id: Id,
        permissions: &[TopicPermissionType],
    ) -> NotifyExchangeResult<bool> {
        let res = TopicPermissionDsl::find()
            .filter(TopicPermissionColumn::TopicId.eq(topic_id))
            .filter(TopicPermissionColumn::UserId.eq(user_id))
            .filter(TopicPermissionColumn::PermissionType.is_in(permissions.to_vec()))
            .count(conn)
            .await?;
        Ok(res > 0)
    }

    pub async fn has_manage_permissions<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        topic_id: Id,
        user_id: Id,
    ) -> NotifyExchangeResult<bool> {
        self.has_any_permissions(
            conn,
            topic_id,
            user_id,
            &[
                TopicPermissionType::ManageRead,
                TopicPermissionType::ManageWrite,
                TopicPermissionType::ManageReadInPublic,
            ],
        )
        .await
    }

    pub async fn has_permissions<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        topic_id: Id,
        user_id: Id,
        permissions: &[TopicPermissionType],
    ) -> NotifyExchangeResult<bool> {
        let res = TopicPermissionDsl::find()
            .filter(TopicPermissionColumn::TopicId.eq(topic_id))
            .filter(TopicPermissionColumn::UserId.eq(user_id))
            .filter(TopicPermissionColumn::PermissionType.is_in(permissions.to_vec()))
            .all(conn)
            .await?;
        for p in permissions {
            if res.iter().all(|r| r.permission_type != *p) {
                return Ok(false);
            }
        }

        Ok(true)
    }

    pub async fn find_user_topic_permissions<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        topic_id: Id,
        user_id: Id,
    ) -> NotifyExchangeResult<Vec<TopicPermission>> {
        let res = TopicPermissionDsl::find()
            .filter(TopicPermissionColumn::TopicId.eq(topic_id))
            .filter(TopicPermissionColumn::UserId.eq(user_id))
            .all(conn)
            .await?;

        Ok(res)
    }

    pub async fn list_pagination_topic_user_permissions<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        topic_id: Id,
        offset: usize,
        limit: usize,
    ) -> NotifyExchangeResult<Vec<(User, Vec<TopicPermissionType>)>> {
        self.find_by_id(conn, topic_id).await?;

        let user_topics = UserTopicDsl::find()
            .filter(UserTopicColumn::TopicId.eq(topic_id))
            .order_by_desc(UserTopicColumn::CreatedAt)
            .offset(offset as u64)
            .limit(limit as u64)
            .all(conn)
            .await?;
        let user_ids = user_topics.iter().map(|ut| ut.user_id).collect::<Vec<_>>();

        let users = UserDsl::find()
            .filter(UserColumn::Id.is_in(user_ids.clone()))
            .all(conn)
            .await?
            .into_iter()
            .map(|u| (u.id, u))
            .collect::<HashMap<_, _>>();

        let mut permissions_by_user: HashMap<Id, Vec<TopicPermissionType>> = HashMap::new();
        for (user_id, permission_type) in TopicPermissionDsl::find()
            .select_only()
            .column(TopicPermissionColumn::UserId)
            .column(TopicPermissionColumn::PermissionType)
            .filter(TopicPermissionColumn::TopicId.eq(topic_id))
            .filter(TopicPermissionColumn::UserId.is_in(user_ids))
            .into_tuple::<(Id, TopicPermissionType)>()
            .all(conn)
            .await?
        {
            permissions_by_user
                .entry(user_id)
                .or_default()
                .push(permission_type);
        }

        Ok(user_topics
            .into_iter()
            .filter_map(|ut| {
                users.get(&ut.user_id).cloned().map(|user| {
                    (
                        user,
                        permissions_by_user.remove(&ut.user_id).unwrap_or_default(),
                    )
                })
            })
            .collect())
    }

    pub async fn set_user_topic_permissions(
        &self,
        conn: &DatabaseTransaction,
        operator_id: Id,
        topic_id: Id,
        user_id: Id,
        new_permissions: Vec<TopicPermissionType>,
        assign_new_user: bool,
    ) -> NotifyExchangeResult<()> {
        let topic = self.find_by_id(conn, topic_id).await?;
        if topic.user_id == user_id {
            return Err(error::forbidden(
                "The topic permissions of the owner can't be changed",
            ));
        }

        let is_admin = !self
            .context
            .user_service()
            .is_admin(conn, operator_id)
            .await?;

        let now = Utc::now();
        let user_topic = UserTopicDsl::find()
            .filter(
                Cond::all()
                    .add(UserTopicColumn::TopicId.eq(topic_id))
                    .add(UserTopicColumn::UserId.eq(user_id)),
            )
            .one(conn)
            .await?;

        if new_permissions.is_empty() {
            if assign_new_user {
                return Err(error::invalid_request(
                    "It assigns permissions to a new user, but no permission is provided",
                ));
            }
            if !is_admin {
                let mut requires = HashSet::new();
                let user_permissions = self
                    .find_user_topic_permissions(conn, topic_id, user_id)
                    .await?;
                for up in user_permissions {
                    requires.insert(up.permission_type.require());
                }
                // check if the operator has permission
                if !self
                    .has_permissions(
                        conn,
                        topic_id,
                        operator_id,
                        &requires.iter().copied().collect::<Vec<_>>(),
                    )
                    .await?
                {
                    tracing::warn!("It needs permissions: {requires:?}, but not enough");
                    return Err(error::forbidden("No permission"));
                }
            }
            let res = UserTopicDsl::delete_many()
                .filter(UserTopicColumn::TopicId.eq(topic_id))
                .filter(UserTopicColumn::UserId.eq(user_id))
                .exec(conn)
                .await?;
            tracing::info!(
                "Delete all {} permissions of user[{user_id}] for topic[{topic_id}]",
                res.rows_affected,
            );
            if let Some(user_topic) = user_topic {
                user_topic.into_active_model().delete(conn).await?;
            }
            Ok(())
        } else {
            if user_topic.is_none() {
                // check if user_id are valid
                self.context
                    .user_service()
                    .find_user_by_id(conn, user_id)
                    .await?;
                // insert a user topic record
                UserTopic {
                    id: Id::new(),
                    topic_id,
                    user_id,
                    created_at: now,
                    updated_at: now,
                }
                .into_active_model()
                .insert(conn)
                .await?;
            } else if assign_new_user {
                return Err(error::invalid_request(
                    "It assigns permissions to a new user, but the user has permissions for the topic already",
                ));
            }

            let mut requires = HashSet::new();
            let user_permissions = self
                .find_user_topic_permissions(conn, topic_id, user_id)
                .await?;
            let mut deletes = vec![];
            for up in &user_permissions {
                if !new_permissions.contains(&up.permission_type) {
                    deletes.push(up.id);
                    requires.insert(up.permission_type.require());
                }
            }
            let mut creates = HashSet::new();
            for new_p in new_permissions {
                if user_permissions
                    .iter()
                    .all(|up| up.permission_type != new_p)
                {
                    creates.insert(new_p);
                    requires.insert(new_p.require());
                }
            }

            // check if the operator has permission
            if !self
                .has_permissions(
                    conn,
                    topic_id,
                    operator_id,
                    &requires.iter().copied().collect::<Vec<_>>(),
                )
                .await?
            {
                tracing::warn!("It needs permissions: {requires:?}, but not enough");
                return Err(error::forbidden("No permission"));
            }

            let new_permissions = creates
                .iter()
                .map(|&permission_type| {
                    TopicPermission {
                        id: Id::new(),
                        topic_id,
                        user_id,
                        permission_type,
                        created_at: now,
                        updated_at: now,
                    }
                    .into_active_model()
                })
                .collect::<Vec<_>>();

            let deleted_row = if !deletes.is_empty() {
                TopicPermissionDsl::delete_many()
                    .filter(Cond::all().add(TopicPermissionColumn::Id.is_in(deletes)))
                    .exec(conn)
                    .await?
                    .rows_affected
            } else {
                0
            };
            if !new_permissions.is_empty() {
                TopicPermissionDsl::insert_many(new_permissions)
                    .exec(conn)
                    .await?;
            }
            tracing::info!(
                "Update permissions of user[{user_id}] for topic[{topic_id}], new permissions: {creates:?}, deletes: {}",
                deleted_row
            );
            Ok(())
        }
    }

    pub async fn find_latest_message_by_topic(
        &self,
        topic_id: Id,
    ) -> NotifyExchangeResult<Option<Message>> {
        let res = MessageDsl::find()
            .filter(MessageColumn::TopicId.eq(topic_id))
            .order_by_desc(MessageColumn::Id)
            .one(self.context.msg_db())
            .await?;

        Ok(res)
    }

    pub async fn find_latest_message_id_by_topic(
        &self,
        topic_id: Id,
    ) -> NotifyExchangeResult<Option<i64>> {
        let res = MessageDsl::find()
            .filter(MessageColumn::TopicId.eq(topic_id))
            .select_only()
            .column(MessageColumn::Id)
            .order_by_desc(MessageColumn::Id)
            .into_tuple()
            .one(self.context.msg_db())
            .await?;

        Ok(res)
    }

    /// Find all topics.
    ///
    /// # Returns
    ///
    /// It will return a list of topics, and the last topic id for pagination.
    pub async fn list_topic<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        last_topic_id: Option<Id>,
        limit: usize,
    ) -> NotifyExchangeResult<(Vec<Topic>, Option<Id>)> {
        Ok(TopicDsl::find()
            .filter(Cond::all().add_option(last_topic_id.map(|id| TopicColumn::Id.lt(id))))
            .order_by_desc(TopicColumn::Id)
            .limit(limit as u64)
            .all(conn)
            .await
            .map(|p| FirstPage(limit).of(p))?)
    }

    pub async fn list_pagination_topic<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        cond: Cond,
        order_bys: Vec<(SimpleExpr, Order)>,
        offset: usize,
        limit: usize,
    ) -> NotifyExchangeResult<Vec<Topic>> {
        let mut query = TopicDsl::find().filter(cond);
        for order_by in order_bys {
            query = query.order_by(order_by.0, order_by.1);
        }
        query = query.offset(offset as u64).limit(limit as u64);
        Ok(query.all(conn).await?)
    }

    /// Find topics that the user has any permissions on.
    ///
    /// # Returns
    ///
    /// It will return a list of topics with user's permissions, and the last topic id for pagination.
    pub async fn list_authorized_topic<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        user_id: Id,
        last_topic_id: Option<Id>,
        limit: usize,
    ) -> NotifyExchangeResult<(Vec<(Topic, Vec<TopicPermissionType>)>, Option<Id>)> {
        let (topics, last_topic_id) = TopicDsl::find()
            .reverse_join(UserTopicDsl)
            .filter(
                Cond::all()
                    .add(UserTopicColumn::UserId.eq(user_id))
                    .add_option(last_topic_id.map(|id| UserTopicColumn::Id.lt(id))),
            )
            .order_by_desc(UserTopicColumn::Id)
            .limit(limit as u64)
            .all(conn)
            .await
            .map(|p| FirstPage(limit).of(p))?;

        let mut topic_with_permission_list = Vec::with_capacity(topics.len());

        for topic in topics {
            let topic_permission_types = TopicPermissionDsl::find()
                .select_only()
                .column(TopicPermissionColumn::PermissionType)
                .filter(
                    Cond::all()
                        .add(TopicPermissionColumn::TopicId.eq(topic.id))
                        .add(TopicPermissionColumn::UserId.eq(user_id)),
                )
                .into_tuple()
                .all(conn)
                .await?;
            topic_with_permission_list.push((topic, topic_permission_types));
        }

        Ok((topic_with_permission_list, last_topic_id))
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn list_pagination_authorized_topic<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        user_id: Id,
        subscribed: Option<bool>,
        cond: Cond,
        order_bys: Vec<(SimpleExpr, Order)>,
        offset: usize,
        limit: usize,
    ) -> NotifyExchangeResult<Vec<Topic>> {
        let mut query = TopicDsl::find()
            .reverse_join(UserTopicDsl)
            .filter(cond.add(UserTopicColumn::UserId.eq(user_id)));
        if let Some(subscribed) = subscribed {
            let sub_query = QueryTrait::query(
                &mut SubscriptionDsl::find()
                    .select_only()
                    .column(SubscriptionColumn::TopicId)
                    .filter(Cond::all().add(SubscriptionColumn::UserId.eq(user_id)))
                    .group_by(SubscriptionColumn::TopicId),
            )
            .take();
            if subscribed {
                query = query.filter(TopicColumn::Id.in_subquery(sub_query));
            } else {
                query = query.filter(TopicColumn::Id.not_in_subquery(sub_query));
            }
        }
        for order_by in order_bys {
            query = query.order_by(order_by.0, order_by.1);
        }
        query = query.offset(offset as u64).limit(limit as u64);
        Ok(query.all(conn).await?)
    }

    pub async fn find_topics_with_latest_message_id<Conn: ConnectionTrait, I>(
        &self,
        conn: &Conn,
        topic_ids: I,
    ) -> NotifyExchangeResult<Vec<(Topic, i64)>>
    where
        I: IntoIterator<Item = Id> + Clone,
    {
        let topics = TopicDsl::find()
            .filter(Cond::all().add(TopicColumn::Id.is_in(topic_ids.clone())))
            .all(conn)
            .await?;
        let latest_message_ids: HashMap<_, _> = MessageDsl::find()
            .filter(Cond::all().add(MessageColumn::TopicId.is_in(topic_ids)))
            .select_only()
            .column(MessageColumn::TopicId)
            .column(MessageColumn::Id)
            .into_tuple::<(Id, i64)>()
            .all(self.context.msg_db())
            .await?
            .into_iter()
            .collect();

        let mut res = Vec::with_capacity(topics.len());
        for topic in topics {
            let latest_message_id = latest_message_ids.get(&topic.id).copied().unwrap_or(0);
            res.push((topic, latest_message_id));
        }

        Ok(res)
    }

    pub async fn find_latest_message_ids<I>(
        &self,
        topic_ids: I,
    ) -> NotifyExchangeResult<HashMap<Id, i64>>
    where
        I: IntoIterator<Item = Id>,
    {
        Ok(MessageDsl::find()
            .filter(Cond::all().add(MessageColumn::TopicId.is_in(topic_ids)))
            .select_only()
            .column(MessageColumn::TopicId)
            .column(MessageColumn::Id)
            .into_tuple::<(Id, i64)>()
            .all(self.context.msg_db())
            .await?
            .into_iter()
            .collect())
    }

    pub async fn list_topic_message<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        topic_id: Id,
        user_id: Id,
        last_message_id: Option<i64>,
        limit: usize,
    ) -> NotifyExchangeResult<(Vec<Message>, Option<i64>)> {
        if !self
            .has_permissions(conn, topic_id, user_id, &[TopicPermissionType::Read])
            .await?
            && !self
                .has_permissions(
                    conn,
                    topic_id,
                    user_id,
                    &[TopicPermissionType::ReadInPublic],
                )
                .await?
        {
            return Err(error::forbidden("No permission to read"));
        }
        Ok(MessageDsl::find()
            .filter(
                Cond::all()
                    .add(MessageColumn::TopicId.eq(topic_id))
                    .add_option(last_message_id.map(|id| MessageColumn::Id.lt(id))),
            )
            .order_by_desc(MessageColumn::Id)
            .all(self.context.msg_db())
            .await
            .map(|p| FirstPage(limit).of(p))?)
    }
}

pub struct SubscriptionService<'a> {
    context: &'a Context,
}

impl<'a> SubscriptionService<'a> {
    pub async fn find_subscription_and_offset<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        topic_id: Id,
        endpoint_id: Id,
    ) -> NotifyExchangeResult<Option<(Subscription, SubscriptionOffset)>> {
        let res = SubscriptionDsl::find()
            .filter(SubscriptionColumn::TopicId.eq(topic_id))
            .filter(SubscriptionColumn::EndpointId.eq(endpoint_id))
            .find_also_related(SubscriptionOffsetDsl)
            .one(conn)
            .await?;
        if let Some((sub, sub_offset)) = res {
            let Some(sub_offset) = sub_offset else {
                return Err(error::internal_server_error(format!(
                    "Subscription[{}] without offset record",
                    sub.id
                )));
            };
            Ok(Some((sub, sub_offset)))
        } else {
            Ok(None)
        }
    }

    pub async fn find_subscription<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        topic_id: Id,
        endpoint_id: Id,
    ) -> NotifyExchangeResult<Option<Subscription>> {
        Ok(SubscriptionDsl::find()
            .filter(SubscriptionColumn::TopicId.eq(topic_id))
            .filter(SubscriptionColumn::EndpointId.eq(endpoint_id))
            .one(conn)
            .await?)
    }

    pub async fn find_offset<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        subscription_id: Id,
    ) -> NotifyExchangeResult<SubscriptionOffset> {
        let Some(sub_offset) = SubscriptionOffsetDsl::find_by_id(subscription_id)
            .one(conn)
            .await?
        else {
            return Err(error::internal_server_error(format!(
                "Subscription[{subscription_id}] without offset record",
            )));
        };
        Ok(sub_offset)
    }

    pub async fn commit<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        endpoint: &Endpoint,
        subscription_id: Id,
        offset: i64,
    ) -> NotifyExchangeResult<SubscriptionOffset> {
        let subscription = SubscriptionDsl::find_by_id(subscription_id)
            .filter(SubscriptionColumn::EndpointId.eq(endpoint.id))
            .one(conn)
            .await?;
        if subscription.is_none() {
            return Err(error::not_found("No subscription"));
        }
        let new_offset = ActiveSubscriptionOffset {
            subscription_id: ActiveValue::Unchanged(subscription_id),
            next_message_id: ActiveValue::Set(offset + 1),
            ..Default::default()
        }
        .update(conn)
        .await?;
        Ok(new_offset)
    }

    async fn insert(
        &self,
        conn: &DatabaseTransaction,
        topic_id: Id,
        endpoint_id: Id,
        subscriber_id: Id,
        next_message_id: i64,
    ) -> NotifyExchangeResult<Subscription> {
        let now = Utc::now();
        let subscription = Subscription {
            id: Id::new(),
            topic_id,
            endpoint_id,
            user_id: subscriber_id,
            status: SubscriptionStatus::On,
            description: Default::default(),
            created_at: now,
            updated_at: now,
        }
        .into_active_model()
        .insert(conn)
        .await?;

        // Create the offset record
        SubscriptionOffset {
            subscription_id: subscription.id,
            next_message_id,
            created_at: now,
            updated_at: now,
        }
        .into_active_model()
        .insert(conn)
        .await?;

        self.context.dispatcher_manager().load(subscription.id);
        Ok(subscription)
    }

    async fn delete(
        &self,
        conn: &DatabaseTransaction,
        subscription_id: Id,
    ) -> NotifyExchangeResult<()> {
        SubscriptionOffsetDsl::delete_by_id(subscription_id)
            .exec(conn)
            .await?;
        SubscriptionDsl::delete_by_id(subscription_id)
            .exec(conn)
            .await?;
        self.context.dispatcher_manager().unload(subscription_id);
        Ok(())
    }

    async fn delete_by_topic_id(
        &self,
        conn: &DatabaseTransaction,
        topic_id: Id,
    ) -> NotifyExchangeResult<()> {
        let ids = SubscriptionDsl::find()
            .filter(SubscriptionColumn::TopicId.eq(topic_id))
            .select_only()
            .column(SubscriptionColumn::Id)
            .into_tuple()
            .all(conn)
            .await?;
        for chunk_ids in ids.chunks(20) {
            SubscriptionOffsetDsl::delete_many()
                .filter(SubscriptionOffsetColumn::SubscriptionId.is_in(chunk_ids.to_vec()))
                .exec(conn)
                .await?;
        }
        SubscriptionDsl::delete_many()
            .filter(SubscriptionColumn::TopicId.eq(topic_id))
            .exec(conn)
            .await?;
        for id in ids {
            self.context.dispatcher_manager().unload(id);
        }
        Ok(())
    }

    pub async fn delete_by_endpoint_id(
        &self,
        conn: &DatabaseTransaction,
        endpoint_id: Id,
    ) -> NotifyExchangeResult<()> {
        let ids = SubscriptionDsl::find()
            .filter(SubscriptionColumn::EndpointId.eq(endpoint_id))
            .select_only()
            .column(SubscriptionColumn::Id)
            .into_tuple()
            .all(conn)
            .await?;
        for chunk_ids in ids.chunks(20) {
            SubscriptionOffsetDsl::delete_many()
                .filter(SubscriptionOffsetColumn::SubscriptionId.is_in(chunk_ids.to_vec()))
                .exec(conn)
                .await?;
        }
        SubscriptionDsl::delete_many()
            .filter(SubscriptionColumn::EndpointId.eq(endpoint_id))
            .exec(conn)
            .await?;
        for id in ids {
            self.context.dispatcher_manager().unload(id);
        }
        Ok(())
    }

    pub async fn reset_failed_subscription_of_endpoint(
        &self,
        conn: &DatabaseTransaction,
        endpoint: &Endpoint,
    ) -> NotifyExchangeResult<()> {
        SubscriptionDsl::update_many()
            .filter(SubscriptionColumn::EndpointId.eq(endpoint.id))
            .filter(SubscriptionColumn::Status.eq(SubscriptionStatus::OffByError))
            .col_expr(
                SubscriptionColumn::Status,
                Expr::value(SubscriptionStatus::On),
            )
            .exec(conn)
            .await?;
        Ok(())
    }

    pub async fn find_by_id<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        subscription_id: Id,
    ) -> NotifyExchangeResult<Option<Subscription>> {
        Ok(SubscriptionDsl::find_by_id(subscription_id)
            .one(conn)
            .await?)
    }

    pub async fn list_subscription<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        cond: Cond,
        last_subscription_id: Option<Id>,
        limit: usize,
    ) -> NotifyExchangeResult<(Vec<(Subscription, Option<SubscriptionOffset>)>, Option<Id>)> {
        Ok(SubscriptionDsl::find()
            .filter(cond.add_option(last_subscription_id.map(|id| SubscriptionColumn::Id.lt(id))))
            .find_also_related(SubscriptionOffsetDsl)
            .order_by_desc(SubscriptionColumn::Id)
            .limit(limit as u64)
            .all(conn)
            .await
            .map(|p| FirstPage(limit).of(p))?)
    }

    pub async fn list_pagination_subscription<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        cond: Cond,
        order_bys: Vec<(SimpleExpr, Order)>,
        offset: usize,
        limit: usize,
    ) -> NotifyExchangeResult<Vec<(Subscription, Option<SubscriptionOffset>)>> {
        let mut query = SubscriptionDsl::find()
            .filter(cond)
            .find_also_related(SubscriptionOffsetDsl);
        for order_by in order_bys {
            query = query.order_by(order_by.0, order_by.1);
        }
        Ok(query
            .offset(offset as u64)
            .limit(limit as u64)
            .all(conn)
            .await?)
    }

    pub async fn filter_subscribed_topics<Conn: ConnectionTrait, I>(
        &self,
        conn: &Conn,
        user_id: Id,
        topic_ids: I,
    ) -> NotifyExchangeResult<HashSet<Id>>
    where
        I: IntoIterator<Item = Id>,
    {
        Ok(SubscriptionDsl::find()
            .filter(
                Cond::all()
                    .add(SubscriptionColumn::UserId.eq(user_id))
                    .add(SubscriptionColumn::TopicId.is_in(topic_ids)),
            )
            .select_only()
            .column(SubscriptionColumn::TopicId)
            .into_tuple()
            .all(conn)
            .await?
            .into_iter()
            .collect())
    }
}

impl Context {
    pub fn topic_service(&self) -> TopicService<'_> {
        TopicService { context: self }
    }

    pub fn subscription_service(&self) -> SubscriptionService<'_> {
        SubscriptionService { context: self }
    }
}

type SignalMap = HashMap<Id, (WatchSender<i64>, WatchReceiver<i64>)>;

#[derive(Default)]
pub struct IncomingMessageSignals {
    signals: Mutex<SignalMap>,
}

impl IncomingMessageSignals {
    pub async fn new_message(&self, topic_id: Id, offset: i64) {
        let mut signals = self.signals.lock().await;
        let entry = signals.entry(topic_id).or_insert_with(|| watch::channel(0));
        if entry.0.send(offset).is_err() {
            tracing::error!("No watch receiver for topic[{topic_id}]");
        }
    }

    pub async fn wait_new_message(&self, topic_id: Id) -> Result<i64, ()> {
        let mut signals = self.signals.lock().await;
        let entry = signals.entry(topic_id).or_insert_with(|| watch::channel(0));
        let mut watcher = entry.1.clone();
        // Avoid locking other tasks to call this method
        drop(signals);
        if watcher.changed().await.is_err() {
            Err(())
        } else {
            let mut signals = self.signals.lock().await;
            if let Some(entry) = signals.get_mut(&topic_id) {
                // Update the base receiver version
                Ok(*entry.1.borrow_and_update())
            } else {
                tracing::warn!("Topic[{}] is removed", topic_id);
                Err(())
            }
        }
    }

    pub async fn wait_new_message_with_timeout(
        &self,
        topic_id: Id,
        timeout: Duration,
    ) -> Option<Result<i64, ()>> {
        tokio::select! {
            offset = self.wait_new_message(topic_id) => {
                Some(offset)
            },
            _ = tokio::time::sleep(timeout) => {
                None
            },
        }
    }

    pub async fn remove_topic(&self, topic_id: Id) -> bool {
        let mut signals = self.signals.lock().await;
        signals.remove(&topic_id).is_some()
    }
}

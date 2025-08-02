use chrono::Utc;
use sea_orm::{
    DeriveRelation, EntityTrait as _, EnumIter, LinkDef, Linked, Related, RelationDef,
    RelationTrait as _, prelude::DateTimeUtc,
};

use crate::{db::custom_type::Id, service::secret::SecretLike};

mod generated;

macro_rules! pub_use_and_alias {
    ($mod:path, $name:ident) => {
        paste::paste! {
            pub use generated::$mod::ActiveModel as [<Active $name>];
            pub use generated::$mod::Column as [<$name Column>];
            pub use generated::$mod::Entity as [<$name Dsl>];
            pub use generated::$mod::Model as $name;
        }
    };
}

pub_use_and_alias!(cache, Cache);
pub_use_and_alias!(endpoint, Endpoint);
pub_use_and_alias!(endpoint_secret, EndpointSecret);
pub_use_and_alias!(lock, Lock);
pub_use_and_alias!(message, Message);
pub_use_and_alias!(pending_user, PendingUser);
pub_use_and_alias!(role, Role);
pub_use_and_alias!(secret, Secret);
pub_use_and_alias!(subscription, Subscription);
pub_use_and_alias!(subscription_offset, SubscriptionOffset);
pub_use_and_alias!(topic_permission, TopicPermission);
pub_use_and_alias!(topic, Topic);
pub_use_and_alias!(transport_service, TransportService);
pub_use_and_alias!(transport_service_secret, TransportServiceSecret);
pub_use_and_alias!(user_role, UserRole);
pub_use_and_alias!(user, User);
pub_use_and_alias!(user_topic, UserTopic);

pub trait MaybeExpirable {
    fn expired_at(&self) -> Option<&DateTimeUtc>;

    fn is_expired(&self) -> bool {
        self.is_expired_at(&Utc::now())
    }

    fn is_expired_at(&self, target: &DateTimeUtc) -> bool {
        tracing::debug!("expired_at: {:?}, target: {target}", self.expired_at());
        self.expired_at().filter(|e| *e < target).is_some()
    }
}

macro_rules! impl_expirable {
    ($ty:path) => {
        impl MaybeExpirable for $ty {
            fn expired_at(&self) -> Option<&DateTimeUtc> {
                Some(&self.expired_at)
            }
        }
    };
}

macro_rules! impl_maybe_expirable {
    ($ty:path) => {
        impl MaybeExpirable for $ty {
            fn expired_at(&self) -> Option<&DateTimeUtc> {
                self.expired_at.as_ref()
            }
        }
    };
}

impl_expirable!(PendingUser);

impl_maybe_expirable!(Cache);
impl_maybe_expirable!(EndpointSecret);
impl_maybe_expirable!(TransportServiceSecret);

pub trait ExtractId {
    type IdType;

    fn id(&self) -> Self::IdType;
}

macro_rules! impl_extract_id {
    ($ty:path, $id_ty:path) => {
        impl ExtractId for $ty {
            type IdType = $id_ty;

            fn id(&self) -> Self::IdType {
                self.id
            }
        }
    };
}

impl_extract_id!(Endpoint, Id);
impl_extract_id!(EndpointSecret, Id);
impl_extract_id!(Message, i64);
impl_extract_id!(PendingUser, Id);
impl_extract_id!(Subscription, Id);
impl_extract_id!(TransportService, Id);
impl_extract_id!(Topic, Id);
impl_extract_id!(User, Id);

impl ExtractId for (Subscription, Option<SubscriptionOffset>) {
    type IdType = Id;

    fn id(&self) -> Self::IdType {
        self.0.id
    }
}

pub trait Pagable: Sized {
    type TokenType;

    fn next_page_token(&self, page_size: usize) -> Option<Self::TokenType>;

    fn split_at(self, page_size: usize) -> (Self, Self);
}

impl<T> Pagable for Vec<T>
where
    T: ExtractId,
{
    type TokenType = <T as ExtractId>::IdType;

    fn next_page_token(&self, page_size: usize) -> Option<Self::TokenType> {
        if self.is_empty() || page_size == 0 {
            None
        } else {
            self.get(page_size - 1).map(ExtractId::id)
        }
    }

    fn split_at(mut self, page_size: usize) -> (Self, Self) {
        if self.len() <= page_size {
            (self, vec![])
        } else {
            let other = self.split_off(page_size);
            (self, other)
        }
    }
}

pub struct FirstPage(pub usize);

impl FirstPage {
    pub fn of<P, T>(&self, p: P) -> (P, Option<T>)
    where
        P: Pagable<TokenType = T>,
    {
        let token = p.next_page_token(self.0);
        (p.split_at(self.0).0, token)
    }
}

impl Role {
    pub const ADMIN: &'static str = "admin";
    pub const USER: &'static str = "user";

    pub fn is_admin(&self) -> bool {
        self.code == Self::ADMIN
    }
}

impl SubscriptionOffset {
    pub fn committed_offset(&self) -> i64 {
        if self.next_message_id == 0 {
            0
        } else {
            self.next_message_id - 1
        }
    }
}

impl SecretLike for Secret {
    fn encrypt_key_code(&self) -> &str {
        &self.key_code
    }

    fn encrypt_key_source(&self) -> super::custom_type::KeySource {
        self.key_source
    }

    fn encryption_type(&self) -> super::custom_type::EncryptionType {
        self.encryption_type
    }

    fn options(&self) -> &str {
        &self.options
    }

    fn encrypted_data(&self) -> &str {
        &self.data
    }
}

impl SecretLike for Message {
    fn encrypt_key_code(&self) -> &str {
        &self.key_code
    }

    fn encrypt_key_source(&self) -> super::custom_type::KeySource {
        self.key_source
    }

    fn encryption_type(&self) -> super::custom_type::EncryptionType {
        self.encryption_type
    }

    fn options(&self) -> &str {
        &self.options
    }

    fn encrypted_data(&self) -> &str {
        &self.data
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
#[sea_orm(entity = TransportServiceSecretDsl)]
pub enum TransportServiceSecretRelation {
    #[sea_orm(
        belongs_to = "SecretDsl",
        from = "TransportServiceSecretColumn::SecretId"
        to = "SecretColumn::Id",
    )]
    Secret,
}

impl Related<SecretDsl> for TransportServiceSecretDsl {
    fn to() -> RelationDef {
        TransportServiceSecretRelation::Secret.def()
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
#[sea_orm(entity = SecretDsl)]
pub enum SecretRelation {
    #[sea_orm(has_one = "TransportServiceSecretDsl")]
    TransportServiceSecret,
    #[sea_orm(has_one = "EndpointSecretDsl")]
    EndpointSecret,
}

impl Related<TransportServiceSecretDsl> for SecretDsl {
    fn to() -> RelationDef {
        SecretRelation::TransportServiceSecret.def()
    }
}

impl Related<EndpointSecretDsl> for SecretDsl {
    fn to() -> RelationDef {
        SecretRelation::EndpointSecret.def()
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
#[sea_orm(entity = UserRoleDsl)]
pub enum UserRoleRelation {
    #[sea_orm(
        belongs_to = "UserDsl",
        from = "UserRoleColumn::UserId"
        to = "UserColumn::Id",
    )]
    User,
    #[sea_orm(
        belongs_to = "RoleDsl",
        from = "UserRoleColumn::RoleCode"
        to = "RoleColumn::Code",
    )]
    Role,
}

impl Related<UserDsl> for UserRoleDsl {
    fn to() -> RelationDef {
        UserRoleRelation::User.def()
    }
}

impl Related<RoleDsl> for UserRoleDsl {
    fn to() -> RelationDef {
        UserRoleRelation::Role.def()
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
#[sea_orm(entity = UserDsl)]
pub enum UserRelation {
    #[sea_orm(has_many = "UserRoleDsl")]
    UserRole,
}

impl Related<UserRoleDsl> for UserDsl {
    fn to() -> RelationDef {
        UserRelation::UserRole.def()
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
#[sea_orm(entity = RoleDsl)]
pub enum RoleRelation {
    #[sea_orm(has_many = "UserRoleDsl")]
    UserRole,
}

impl Related<UserRoleDsl> for RoleDsl {
    fn to() -> RelationDef {
        RoleRelation::UserRole.def()
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
#[sea_orm(entity = EndpointSecretDsl)]
pub enum EndpointSecretRelation {
    #[sea_orm(
        belongs_to = "SecretDsl",
        from = "EndpointSecretColumn::SecretId"
        to = "SecretColumn::Id",
    )]
    Secret,
}

impl Related<SecretDsl> for EndpointSecretDsl {
    fn to() -> RelationDef {
        EndpointSecretRelation::Secret.def()
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
#[sea_orm(entity = EndpointDsl)]
pub enum EndpointRelation {
    #[sea_orm(has_many = "SubscriptionDsl")]
    Subscription,
}

impl Related<SubscriptionDsl> for EndpointDsl {
    fn to() -> RelationDef {
        EndpointRelation::Subscription.def()
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
#[sea_orm(entity = SubscriptionDsl)]
pub enum SubscriptionRelation {
    #[sea_orm(has_one = "SubscriptionOffsetDsl")]
    SubscriptionOffset,
    #[sea_orm(
        belongs_to = "EndpointDsl",
        from = "SubscriptionColumn::EndpointId"
        to = "EndpointColumn::Id",
    )]
    Endpoint,
}

impl Related<SubscriptionOffsetDsl> for SubscriptionDsl {
    fn to() -> RelationDef {
        SubscriptionRelation::SubscriptionOffset.def()
    }
}

impl Related<EndpointDsl> for SubscriptionDsl {
    fn to() -> RelationDef {
        SubscriptionRelation::Endpoint.def()
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
#[sea_orm(entity = SubscriptionOffsetDsl)]
pub enum SubscriptionOffsetRelation {
    #[sea_orm(
        belongs_to = "SubscriptionDsl",
        from = "SubscriptionOffsetColumn::SubscriptionId"
        to = "SubscriptionColumn::Id",
    )]
    Subscription,
}

impl Related<SubscriptionDsl> for SubscriptionOffsetDsl {
    fn to() -> RelationDef {
        SubscriptionOffsetRelation::Subscription.def()
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
#[sea_orm(entity = TopicDsl)]
pub enum TopicRelation {
    #[sea_orm(has_many = "TopicPermissionDsl")]
    TopicPermission,
    #[sea_orm(has_many = "UserTopicDsl")]
    UserTopic,
}

impl Related<TopicPermissionDsl> for TopicDsl {
    fn to() -> RelationDef {
        TopicRelation::TopicPermission.def()
    }
}

impl Related<UserTopicDsl> for TopicDsl {
    fn to() -> RelationDef {
        TopicRelation::UserTopic.def()
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
#[sea_orm(entity = TopicPermissionDsl)]
pub enum TopicPermissionRelation {
    #[sea_orm(
        belongs_to = "TopicDsl",
        from = "TopicPermissionColumn::TopicId"
        to = "TopicColumn::Id",
    )]
    Topic,
}

impl Related<TopicDsl> for TopicPermissionDsl {
    fn to() -> RelationDef {
        TopicPermissionRelation::Topic.def()
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
#[sea_orm(entity = UserTopicDsl)]
pub enum UserTopicRelation {
    #[sea_orm(
        belongs_to = "TopicDsl",
        from = "UserTopicColumn::TopicId"
        to = "TopicColumn::Id",
    )]
    Topic,
}

impl Related<TopicDsl> for UserTopicDsl {
    fn to() -> RelationDef {
        UserTopicRelation::Topic.def()
    }
}

pub struct UserToRoleLink;

impl Linked for UserToRoleLink {
    type FromEntity = UserDsl;

    type ToEntity = RoleDsl;

    fn link(&self) -> Vec<LinkDef> {
        vec![
            UserRoleRelation::User.def().rev(),
            UserRoleRelation::Role.def(),
        ]
    }
}

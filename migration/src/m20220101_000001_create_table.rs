use sea_orm_migration::prelude::*;

use crate::ColumnDefExt;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Secret table
        self.create_secret(manager).await?;

        // TransportService table
        self.create_transport_service(manager).await?;

        // TransportServiceSecret table
        self.create_transport_service_secret(manager).await?;

        // Endpoint table
        self.create_endpoint(manager).await?;

        // EndpointSecret table
        self.create_endpoint_secret(manager).await?;

        // Topic table
        self.create_topic(manager).await?;

        // TopicPermission table
        self.create_topic_permission(manager).await?;

        // Subscription table
        self.create_subscription(manager).await?;

        // User table
        self.create_user(manager).await?;

        // PendingUser table
        self.create_pending_user(manager).await?;

        // Role table
        self.create_role(manager).await?;

        // UserRole table
        self.create_user_role(manager).await?;

        // Lock table
        self.create_lock(manager).await?;

        // Message table
        self.create_message(manager).await?;

        // SubscriptionOffset table
        self.create_subscription_offset(manager).await?;

        // Cache table
        self.create_cache(manager).await?;

        // Insert initial role data
        manager
            .get_connection()
            .execute_unprepared(
                r#"INSERT INTO role (code, name, description) VALUES
            ('admin', 'Administrator', 'The administrator role with full access to the system.'),
            ('user', 'User', 'A regular user role with limited access to the system.');"#,
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Cache::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(SubscriptionOffset::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Message::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Lock::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(UserRole::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Role::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(PendingUser::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(User::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Subscription::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(TopicPermission::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Topic::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(EndpointSecret::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Endpoint::Table).to_owned())
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(TransportServiceSecret::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(TransportService::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Secret::Table).to_owned())
            .await?;

        Ok(())
    }
}

impl Migration {
    async fn create_secret(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Secret::Table)
                    .col(
                        ColumnDef::new(Secret::Id)
                            .binary_len(16)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Secret::Code)
                            .string_len(128)
                            .case_sensitive(manager)
                            .not_null()
                            .unique_key(),
                    )
                    .col(
                        ColumnDef::new(Secret::EncryptionType)
                            .tiny_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Secret::KeySource)
                            .tiny_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Secret::KeyCode)
                            .string_len(128)
                            .case_sensitive(manager)
                            .not_null(),
                    )
                    .col(ColumnDef::new(Secret::Options).text().not_null())
                    .col(ColumnDef::new(Secret::Data).text().not_null())
                    .col(ColumnDef::new(Secret::Description).text())
                    .col(
                        ColumnDef::new(Secret::CreatedAt)
                            .timestamp()
                            .extra("DEFAULT CURRENT_TIMESTAMP")
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Secret::UpdatedAt)
                            .timestamp()
                            .extra("DEFAULT CURRENT_TIMESTAMP")
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_s_ca")
                    .table(Secret::Table)
                    .col(Secret::CreatedAt)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_s_kc")
                    .table(Secret::Table)
                    .col(Secret::KeyCode)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn create_transport_service(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(TransportService::Table)
                    .col(
                        ColumnDef::new(TransportService::Id)
                            .binary_len(16)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(TransportService::Name)
                            .string_len(128)
                            .case_sensitive(manager)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TransportService::TransportServiceType)
                            .small_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(TransportService::UserId)
                            .binary_len(16)
                            .not_null(),
                    )
                    .col(ColumnDef::new(TransportService::Options).text().not_null())
                    .col(
                        ColumnDef::new(TransportService::Enabled)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .col(ColumnDef::new(TransportService::Description).text())
                    .col(
                        ColumnDef::new(TransportService::CreatedAt)
                            .timestamp()
                            .extra("DEFAULT CURRENT_TIMESTAMP")
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TransportService::UpdatedAt)
                            .timestamp()
                            .extra("DEFAULT CURRENT_TIMESTAMP")
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_ts_ca")
                    .table(TransportService::Table)
                    .col(TransportService::CreatedAt)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_ts_ui")
                    .table(TransportService::Table)
                    .col(TransportService::UserId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn create_transport_service_secret(
        &self,
        manager: &SchemaManager<'_>,
    ) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(TransportServiceSecret::Table)
                    .col(
                        ColumnDef::new(TransportServiceSecret::Id)
                            .binary_len(16)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(TransportServiceSecret::Code)
                            .string_len(128)
                            .case_sensitive(manager)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TransportServiceSecret::SecretType)
                            .string_len(32)
                            .case_sensitive(manager)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TransportServiceSecret::TransportServiceId)
                            .binary_len(16)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TransportServiceSecret::Version)
                            .tiny_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(TransportServiceSecret::SecretId)
                            .binary_len(16)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TransportServiceSecret::Enabled)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .col(
                        ColumnDef::new(TransportServiceSecret::CreatedAt)
                            .timestamp()
                            .extra("DEFAULT CURRENT_TIMESTAMP")
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TransportServiceSecret::UpdatedAt)
                            .timestamp()
                            .extra("DEFAULT CURRENT_TIMESTAMP")
                            .not_null(),
                    )
                    .col(ColumnDef::new(TransportServiceSecret::ExpiredAt).timestamp())
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("uniq_tss_tsi_c_v")
                    .table(TransportServiceSecret::Table)
                    .col(TransportServiceSecret::TransportServiceId)
                    .col(TransportServiceSecret::Code)
                    .col(TransportServiceSecret::Version)
                    .unique()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn create_endpoint(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Endpoint::Table)
                    .col(
                        ColumnDef::new(Endpoint::Id)
                            .binary_len(16)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Endpoint::Code)
                            .string_len(128)
                            .case_sensitive(manager)
                            .not_null(),
                    )
                    .col(ColumnDef::new(Endpoint::Name).string_len(128).not_null())
                    .col(
                        ColumnDef::new(Endpoint::TransportServiceId)
                            .binary_len(16)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Endpoint::TransportServiceType)
                            .small_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(ColumnDef::new(Endpoint::UserId).binary_len(16).not_null())
                    .col(ColumnDef::new(Endpoint::Options).text().not_null())
                    .col(ColumnDef::new(Endpoint::Description).text())
                    .col(
                        ColumnDef::new(Endpoint::IsPublic)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(Endpoint::CreatedAt)
                            .timestamp()
                            .extra("DEFAULT CURRENT_TIMESTAMP")
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Endpoint::UpdatedAt)
                            .timestamp()
                            .extra("DEFAULT CURRENT_TIMESTAMP")
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("uniq_e_tsi_c")
                    .table(Endpoint::Table)
                    .col(Endpoint::TransportServiceId)
                    .col(Endpoint::Code)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_e_ui")
                    .table(Endpoint::Table)
                    .col(Endpoint::UserId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_e_ca")
                    .table(Endpoint::Table)
                    .col(Endpoint::CreatedAt)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn create_endpoint_secret(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(EndpointSecret::Table)
                    .col(
                        ColumnDef::new(EndpointSecret::Id)
                            .binary_len(16)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(EndpointSecret::Code)
                            .string_len(128)
                            .case_sensitive(manager)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(EndpointSecret::SecretType)
                            .string_len(32)
                            .case_sensitive(manager)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(EndpointSecret::EndpointId)
                            .binary_len(16)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(EndpointSecret::Version)
                            .tiny_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(EndpointSecret::SecretId)
                            .binary_len(16)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(EndpointSecret::Enabled)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .col(
                        ColumnDef::new(EndpointSecret::CreatedAt)
                            .timestamp()
                            .extra("DEFAULT CURRENT_TIMESTAMP")
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(EndpointSecret::UpdatedAt)
                            .timestamp()
                            .extra("DEFAULT CURRENT_TIMESTAMP")
                            .not_null(),
                    )
                    .col(ColumnDef::new(EndpointSecret::ExpiredAt).timestamp())
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("uniq_es_ei_c_v")
                    .table(EndpointSecret::Table)
                    .col(EndpointSecret::EndpointId)
                    .col(EndpointSecret::Code)
                    .col(EndpointSecret::Version)
                    .unique()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn create_topic(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Topic::Table)
                    .col(
                        ColumnDef::new(Topic::Id)
                            .binary_len(16)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Topic::Code)
                            .string_len(128)
                            .case_sensitive(manager)
                            .not_null(),
                    )
                    .col(ColumnDef::new(Topic::Name).string_len(128).not_null())
                    .col(ColumnDef::new(Topic::UserId).binary_len(16).not_null())
                    .col(ColumnDef::new(Topic::Description).text())
                    .col(
                        ColumnDef::new(Topic::CreatedAt)
                            .timestamp()
                            .extra("DEFAULT CURRENT_TIMESTAMP")
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Topic::UpdatedAt)
                            .timestamp()
                            .extra("DEFAULT CURRENT_TIMESTAMP")
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("uniq_t_ui_c")
                    .table(Topic::Table)
                    .col(Topic::UserId)
                    .col(Topic::Code)
                    .unique()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_t_ct")
                    .table(Topic::Table)
                    .col(Topic::CreatedAt)
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(UserTopic::Table)
                    .col(
                        ColumnDef::new(UserTopic::Id)
                            .binary_len(16)
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(UserTopic::TopicId).binary_len(16).not_null())
                    .col(ColumnDef::new(UserTopic::UserId).binary_len(16).not_null())
                    .col(
                        ColumnDef::new(UserTopic::CreatedAt)
                            .timestamp()
                            .extra("DEFAULT CURRENT_TIMESTAMP")
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(UserTopic::UpdatedAt)
                            .timestamp()
                            .extra("DEFAULT CURRENT_TIMESTAMP")
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("uniq_ut_ui_ti")
                    .table(UserTopic::Table)
                    .col(UserTopic::UserId)
                    .col(UserTopic::TopicId)
                    .unique()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("uniq_ut_ti_ui")
                    .table(UserTopic::Table)
                    .col(UserTopic::TopicId)
                    .col(UserTopic::UserId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn create_topic_permission(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(TopicPermission::Table)
                    .col(
                        ColumnDef::new(TopicPermission::Id)
                            .binary_len(16)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(TopicPermission::TopicId)
                            .binary_len(16)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TopicPermission::UserId)
                            .binary_len(16)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TopicPermission::PermissionType)
                            .tiny_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(TopicPermission::CreatedAt)
                            .timestamp()
                            .extra("DEFAULT CURRENT_TIMESTAMP")
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TopicPermission::UpdatedAt)
                            .timestamp()
                            .extra("DEFAULT CURRENT_TIMESTAMP")
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("uniq_tp_ti_ui_pt")
                    .table(TopicPermission::Table)
                    .col(TopicPermission::TopicId)
                    .col(TopicPermission::UserId)
                    .col(TopicPermission::PermissionType)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_tp_ui_ti")
                    .table(TopicPermission::Table)
                    .col(TopicPermission::UserId)
                    .col(TopicPermission::TopicId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn create_subscription(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Subscription::Table)
                    .col(
                        ColumnDef::new(Subscription::Id)
                            .binary_len(16)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Subscription::TopicId)
                            .binary_len(16)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Subscription::EndpointId)
                            .binary_len(16)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Subscription::UserId)
                            .binary_len(16)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Subscription::Status)
                            .tiny_integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Subscription::Description).text())
                    .col(
                        ColumnDef::new(Subscription::CreatedAt)
                            .timestamp()
                            .extra("DEFAULT CURRENT_TIMESTAMP")
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Subscription::UpdatedAt)
                            .timestamp()
                            .extra("DEFAULT CURRENT_TIMESTAMP")
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("uniq_s_ti_ei")
                    .table(Subscription::Table)
                    .col(Subscription::TopicId)
                    .col(Subscription::EndpointId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_s_ct")
                    .table(Subscription::Table)
                    .col(Subscription::CreatedAt)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn create_user(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(User::Table)
                    .col(
                        ColumnDef::new(User::Id)
                            .binary_len(16)
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(User::Username).string_len(64).not_null())
                    .col(
                        ColumnDef::new(User::Email)
                            .string_len(128)
                            .not_null()
                            .unique_key(),
                    )
                    .col(
                        ColumnDef::new(User::Enabled)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .col(
                        ColumnDef::new(User::CreatedAt)
                            .timestamp()
                            .extra("DEFAULT CURRENT_TIMESTAMP")
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(User::UpdatedAt)
                            .timestamp()
                            .extra("DEFAULT CURRENT_TIMESTAMP")
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_u_ca")
                    .table(User::Table)
                    .col(User::CreatedAt)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn create_pending_user(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(PendingUser::Table)
                    .col(
                        ColumnDef::new(PendingUser::Id)
                            .binary_len(16)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(PendingUser::Username)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(PendingUser::Email)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(ColumnDef::new(PendingUser::Ext).text().not_null())
                    .col(
                        ColumnDef::new(PendingUser::CreatedAt)
                            .timestamp()
                            .extra("DEFAULT CURRENT_TIMESTAMP")
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(PendingUser::ExpiredAt)
                            .timestamp()
                            .extra("DEFAULT CURRENT_TIMESTAMP")
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_pu_ca")
                    .table(PendingUser::Table)
                    .col(PendingUser::CreatedAt)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_pu_e")
                    .table(PendingUser::Table)
                    .col(PendingUser::Email)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_pu_ea")
                    .table(PendingUser::Table)
                    .col(PendingUser::ExpiredAt)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn create_role(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Role::Table)
                    .col(
                        ColumnDef::new(Role::Code)
                            .string_len(64)
                            .case_sensitive(manager)
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Role::Name).string_len(64).not_null())
                    .col(ColumnDef::new(Role::Description).text())
                    .col(
                        ColumnDef::new(Role::CreatedAt)
                            .timestamp()
                            .extra("DEFAULT CURRENT_TIMESTAMP")
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Role::UpdatedAt)
                            .timestamp()
                            .extra("DEFAULT CURRENT_TIMESTAMP")
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn create_user_role(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(UserRole::Table)
                    .col(
                        ColumnDef::new(UserRole::Id)
                            .binary_len(16)
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(UserRole::UserId).binary_len(16).not_null())
                    .col(
                        ColumnDef::new(UserRole::RoleCode)
                            .string_len(64)
                            .case_sensitive(manager)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(UserRole::CreatedAt)
                            .timestamp()
                            .extra("DEFAULT CURRENT_TIMESTAMP")
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(UserRole::UpdatedAt)
                            .timestamp()
                            .extra("DEFAULT CURRENT_TIMESTAMP")
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("uniq_ur_ui_rc")
                    .table(UserRole::Table)
                    .col(UserRole::UserId)
                    .col(UserRole::RoleCode)
                    .unique()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn create_lock(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Lock::Table)
                    .col(
                        ColumnDef::new(Lock::Code)
                            .string_len(128)
                            .case_sensitive(manager)
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Lock::LockType).small_integer().not_null())
                    .col(
                        ColumnDef::new(Lock::CreatedAt)
                            .timestamp()
                            .extra("DEFAULT CURRENT_TIMESTAMP")
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Lock::UpdatedAt)
                            .timestamp()
                            .extra("DEFAULT CURRENT_TIMESTAMP")
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("uniq_l_c_lt")
                    .table(Lock::Table)
                    .col(Lock::Code)
                    .col(Lock::LockType)
                    .unique()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn create_message(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Message::Table)
                    .col(
                        ColumnDef::new(Message::Id)
                            // There is no auto_increment u64 support in sqlite
                            .big_integer()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Message::EncryptionType)
                            .tiny_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Message::KeySource)
                            .tiny_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Message::KeyCode)
                            .string_len(128)
                            .case_sensitive(manager)
                            .not_null(),
                    )
                    .col(ColumnDef::new(Message::Options).text().not_null())
                    .col(ColumnDef::new(Message::TopicId).binary_len(16).not_null())
                    .col(ColumnDef::new(Message::Data).text().not_null())
                    .col(ColumnDef::new(Message::UserId).binary_len(16).not_null())
                    .col(
                        ColumnDef::new(Message::EndpointId)
                            .binary_len(16)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Message::CreatedAt)
                            .timestamp()
                            .extra("DEFAULT CURRENT_TIMESTAMP")
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_m_ca")
                    .table(Message::Table)
                    .col(Message::CreatedAt)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn create_subscription_offset(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(SubscriptionOffset::Table)
                    .col(
                        ColumnDef::new(SubscriptionOffset::SubscriptionId)
                            .binary_len(16)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(SubscriptionOffset::NextMessageId)
                            .big_unsigned()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(SubscriptionOffset::CreatedAt)
                            .timestamp()
                            .extra("DEFAULT CURRENT_TIMESTAMP")
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(SubscriptionOffset::UpdatedAt)
                            .timestamp()
                            .extra("DEFAULT CURRENT_TIMESTAMP")
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_so_ca")
                    .table(SubscriptionOffset::Table)
                    .col(SubscriptionOffset::CreatedAt)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn create_cache(&self, manager: &SchemaManager<'_>) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Cache::Table)
                    .col(
                        ColumnDef::new(Cache::Id)
                            // There is no auto_increment u64 support in sqlite
                            .big_integer()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Cache::Key)
                            .string_len(256)
                            .case_sensitive(manager)
                            .not_null()
                            .unique_key(),
                    )
                    .col(ColumnDef::new(Cache::Value).text().not_null())
                    .col(
                        ColumnDef::new(Cache::CreatedAt)
                            .timestamp()
                            .extra("DEFAULT CURRENT_TIMESTAMP")
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Cache::UpdatedAt)
                            .timestamp()
                            .extra("DEFAULT CURRENT_TIMESTAMP")
                            .not_null(),
                    )
                    .col(ColumnDef::new(Cache::ExpiredAt).timestamp())
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_c_ca")
                    .table(Cache::Table)
                    .col(Cache::CreatedAt)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_c_ea")
                    .table(Cache::Table)
                    .col(Cache::ExpiredAt)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

// Iden enums for all tables
#[derive(Iden)]
enum Secret {
    Table,
    Id,
    Code,
    EncryptionType,
    KeySource,
    KeyCode,
    Options,
    Data,
    Description,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum TransportService {
    Table,
    Id,
    Name,
    #[allow(clippy::enum_variant_names)]
    TransportServiceType,
    UserId,
    Options,
    Enabled,
    Description,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum TransportServiceSecret {
    Table,
    Id,
    Code,
    SecretType,
    TransportServiceId,
    Version,
    SecretId,
    Enabled,
    CreatedAt,
    UpdatedAt,
    ExpiredAt,
}

#[derive(Iden)]
enum Endpoint {
    Table,
    Id,
    Code,
    Name,
    TransportServiceId,
    TransportServiceType,
    UserId,
    Options,
    Description,
    IsPublic,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum EndpointSecret {
    Table,
    Id,
    Code,
    SecretType,
    EndpointId,
    Version,
    SecretId,
    Enabled,
    CreatedAt,
    UpdatedAt,
    ExpiredAt,
}

#[derive(Iden)]
enum Topic {
    Table,
    Id,
    Code,
    Name,
    UserId,
    Description,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum UserTopic {
    Table,
    Id,
    TopicId,
    UserId,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum TopicPermission {
    Table,
    Id,
    TopicId,
    UserId,
    PermissionType,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum Subscription {
    Table,
    Id,
    TopicId,
    EndpointId,
    UserId,
    Status,
    Description,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum User {
    Table,
    Id,
    Username,
    Email,
    Enabled,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum PendingUser {
    Table,
    Id,
    Username,
    Email,
    Ext,
    CreatedAt,
    ExpiredAt,
}

#[derive(Iden)]
enum Role {
    Table,
    Code,
    Name,
    Description,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum UserRole {
    Table,
    Id,
    UserId,
    RoleCode,
    CreatedAt,
    UpdatedAt,
}

/// Used for lock purpose without introducing redis
#[derive(Iden)]
enum Lock {
    Table,
    Code,
    LockType,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum Message {
    Table,
    Id,
    EncryptionType,
    KeySource,
    KeyCode,
    Options,
    TopicId,
    Data,
    UserId,
    EndpointId,
    CreatedAt,
}

#[derive(Iden)]
enum SubscriptionOffset {
    Table,
    SubscriptionId,
    NextMessageId,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum Cache {
    Table,
    Id,
    Key,
    Value,
    CreatedAt,
    UpdatedAt,
    ExpiredAt,
}

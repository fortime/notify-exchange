use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    time::Duration,
};

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait as _, ColumnTrait as _, Condition, ConnectionTrait, EntityTrait as _,
    IntoActiveModel as _, Order, QueryFilter as _, QueryOrder as _, QuerySelect as _,
    SelectColumns as _,
    prelude::Expr,
    sea_query::{Cond, SimpleExpr},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate as _;

use crate::{
    auth,
    context::Context,
    db::{
        custom_type::Id,
        entity::{
            FirstPage, PendingUser, PendingUserColumn, PendingUserDsl, Role, RoleDsl, User,
            UserColumn, UserDsl, UserRole, UserRoleColumn, UserRoleDsl, UserToRoleLink,
        },
    },
    error::{self, NotifyExchangeResult},
    model::{
        cache::{LoginRequestCache, SessionCache},
        req::{CreateEndpointRequest, CreateUserRequest},
    },
};

#[derive(Debug, Deserialize, Serialize, Default)]
pub struct PendingUserRequestExt {
    pub roles: Vec<String>,
    pub endpoints: Vec<CreateEndpointRequest<'static>>,
}

pub struct UserService<'a> {
    context: &'a Context,
}

impl<'a> UserService<'a> {
    pub async fn find_usernames<Conn: ConnectionTrait, I>(
        &self,
        conn: &Conn,
        user_ids: I,
    ) -> NotifyExchangeResult<HashMap<Id, String>>
    where
        I: IntoIterator<Item = Id>,
    {
        Ok(UserDsl::find()
            .filter(Cond::all().add(UserColumn::Id.is_in(user_ids)))
            .select_only()
            .column(UserColumn::Id)
            .column(UserColumn::Username)
            .into_tuple()
            .all(conn)
            .await?
            .into_iter()
            .collect())
    }

    pub async fn list_pending_user<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        condition: Condition,
        last_pending_user_id: Option<Id>,
        limit: usize,
    ) -> NotifyExchangeResult<(Vec<PendingUser>, Option<Id>)> {
        Ok(PendingUserDsl::find()
            .filter(
                condition.add_option(last_pending_user_id.map(|id| PendingUserColumn::Id.lt(id))),
            )
            .order_by_desc(PendingUserColumn::Id)
            .limit(limit as u64)
            .all(conn)
            .await
            .map(|p| FirstPage(limit).of(p))?)
    }

    pub async fn list_pagination_pending_user<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        cond: Cond,
        order_bys: Vec<(SimpleExpr, Order)>,
        offset: usize,
        limit: usize,
    ) -> NotifyExchangeResult<Vec<PendingUser>> {
        let mut query = PendingUserDsl::find().filter(cond);
        for order_by in order_bys {
            query = query.order_by(order_by.0, order_by.1);
        }
        query = query.offset(offset as u64).limit(limit as u64);
        Ok(query.all(conn).await?)
    }

    pub async fn delete_pending_user<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        ids: &[Id],
    ) -> NotifyExchangeResult<u64> {
        Ok(PendingUserDsl::delete_many()
            .filter(Cond::all().add(PendingUserColumn::Id.is_in(ids)))
            .exec(conn)
            .await?
            .rows_affected)
    }

    pub async fn delete_all_pending_user<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
    ) -> NotifyExchangeResult<u64> {
        Ok(PendingUserDsl::delete_many()
            .filter(Cond::all().add(PendingUserColumn::ExpiredAt.lt(Utc::now())))
            .exec(conn)
            .await?
            .rows_affected)
    }

    pub async fn any_pending_user<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
    ) -> NotifyExchangeResult<bool> {
        let res = PendingUserDsl::find()
            .select_column(PendingUserColumn::Id)
            .filter(PendingUserColumn::ExpiredAt.gt(Utc::now()))
            .one(conn)
            .await?;
        Ok(res.is_some())
    }

    pub async fn create_pending_user<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        req: CreateUserRequest<'_>,
        ext: PendingUserRequestExt,
        timeout: Duration,
    ) -> NotifyExchangeResult<PendingUser> {
        let now = Utc::now();
        for endpoint in &ext.endpoints {
            endpoint.validate()?;
        }
        let new_pending_user = PendingUser {
            id: Id::new(),
            username: req.name.to_string(),
            email: req.email.to_string(),
            ext: serde_json::to_string(&ext).map_err(error::serde_json_error_cb(
                "Invalid ext value of pending user",
            ))?,
            created_at: now,
            expired_at: now + timeout,
        };

        let res = new_pending_user.into_active_model().insert(conn).await?;
        Ok(res)
    }

    pub async fn confirm_pending_user<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        pending_user_id: Id,
    ) -> NotifyExchangeResult<(User, PendingUserRequestExt)> {
        let now = Utc::now();

        // Find by id and filter by expired_at, only valid record is returned
        let pending_user = PendingUserDsl::find_by_id(pending_user_id)
            .one(conn)
            .await?
            .ok_or_else(|| error::not_found(format!("No pending user: {pending_user_id}")))?;

        if pending_user.expired_at < now {
            return Err(error::conflict("Pending user is expired"));
        }

        let user = User {
            id: Id::new(),
            username: pending_user.username,
            email: pending_user.email,
            enabled: true,
            created_at: pending_user.created_at,
            updated_at: now,
        }
        .into_active_model()
        .insert(conn)
        .await?;

        // Deserialize the ext field into PendingUserRequestExt
        let ext: PendingUserRequestExt = serde_json::from_str(&pending_user.ext).map_err(
            error::serde_json_error_cb("Invalid ext value of pending user"),
        )?;

        // Remove the pending user from the database
        PendingUserDsl::delete_by_id(pending_user_id)
            .exec(conn)
            .await?;

        // Create a user role for the new user
        self.context
            .role_service()
            .assign_roles(
                conn,
                user.id,
                ext.roles
                    .iter()
                    .map(|s| s.as_str())
                    .chain([Role::USER])
                    .collect(),
            )
            .await?;
        Ok((user, ext))
    }

    pub async fn list_user<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        last_user_id: Option<Id>,
        limit: usize,
    ) -> NotifyExchangeResult<(Vec<User>, Option<Id>)> {
        Ok(UserDsl::find()
            .filter(Cond::all().add_option(last_user_id.map(|id| UserColumn::Id.lt(id))))
            .order_by_desc(UserColumn::Id)
            .limit(limit as u64)
            .all(conn)
            .await
            .map(|p| FirstPage(limit).of(p))?)
    }

    pub async fn list_pagination_user<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        cond: Cond,
        order_bys: Vec<(SimpleExpr, Order)>,
        offset: usize,
        limit: usize,
    ) -> NotifyExchangeResult<Vec<User>> {
        let mut query = UserDsl::find().filter(cond);
        for order_by in order_bys {
            query = query.order_by(order_by.0, order_by.1);
        }
        query = query.offset(offset as u64).limit(limit as u64);
        Ok(query.all(conn).await?)
    }

    pub async fn find_user_by_id<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        user_id: Id,
    ) -> NotifyExchangeResult<User> {
        UserDsl::find_by_id(user_id)
            .one(conn)
            .await?
            .ok_or_else(|| {
                tracing::warn!("User not found: {user_id}");
                error::not_found("User not found")
            })
    }

    pub async fn find_user_by_email<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        email: &str,
    ) -> NotifyExchangeResult<Option<User>> {
        Ok(UserDsl::find()
            .filter(UserColumn::Email.eq(email))
            .one(conn)
            .await?)
    }

    pub async fn find_user_and_roles_by_id<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        user_id: Id,
    ) -> NotifyExchangeResult<Option<(User, Vec<Role>)>> {
        let mut res = UserDsl::find_by_id(user_id)
            .find_with_linked(UserToRoleLink)
            .all(conn)
            .await?;
        if res.is_empty() {
            Ok(None)
        } else {
            Ok(Some(res.swap_remove(0)))
        }
    }

    pub async fn is_admin<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        user_id: Id,
    ) -> NotifyExchangeResult<bool> {
        Ok(UserRoleDsl::find()
            .filter(UserRoleColumn::UserId.eq(user_id))
            .filter(UserRoleColumn::RoleCode.eq(Role::ADMIN))
            .one(conn)
            .await?
            .is_some())
    }

    /// Create a session for the user
    pub async fn login<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        user: Cow<'_, User>,
    ) -> NotifyExchangeResult<(String, String)> {
        let roles = self
            .context
            .role_service()
            .find_roles_by_user(conn, user.id)
            .await?;
        let csrf_token = Uuid::new_v4().to_string();
        let now = Utc::now();
        // Create a session
        let session = SessionCache {
            user: user.into_owned(),
            roles,
            // We don't set the id here, we set it back after auth.
            id: Default::default(),
            csrf_token: csrf_token.clone(),
            updated_at: now,
            expired_at: now + auth::SESSION_TIMEOUT,
        };
        let session_id = self
            .context
            .cache_manager()
            .add_token(&session, auth::SESSION_TIMEOUT, 10)
            .await?;
        Ok((session_id, csrf_token))
    }

    pub async fn login_with_token<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        user: Cow<'_, User>,
        token: &str,
    ) -> NotifyExchangeResult<()> {
        let cache_manager = self.context.cache_manager();

        let Some(mut login_request) = cache_manager.get_token::<LoginRequestCache>(token).await?
        else {
            return Err(error::invalid_request("No such login request"));
        };

        if login_request.user_id != user.id {
            tracing::warn!(
                "Login request[{}] for [{}/{}], but confirmed by [{}/{}]",
                token,
                login_request.user_id,
                login_request.user_email,
                user.id,
                user.email
            );
            return Err(error::invalid_request(
                "Unmatched login request".to_string(),
            ));
        }

        if login_request.session_id.is_some() {
            tracing::warn!(
                "Session has been created for the request[{}], user[{}/{}]",
                token,
                user.id,
                user.email
            );
            return Ok(());
        }

        let (session_id, _) = self.login(conn, user).await?;
        // Write the session id the login request
        login_request.session_id = Some(session_id);
        // Web ui should get the session id in a moment, we can expire the login request soon.
        cache_manager
            .set_named_token(token, &login_request, Some(Duration::from_secs(120)))
            .await?;
        Ok(())
    }

    pub async fn set_user_enabled<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        operator: Cow<'_, User>,
        user_id: Id,
        enabled: bool,
    ) -> NotifyExchangeResult<()> {
        if operator.id == user_id {
            return Err(error::invalid_request(
                "You can't set the enabled field of your own",
            ));
        }
        UserDsl::update_many()
            .filter(UserColumn::Id.eq(user_id))
            .col_expr(UserColumn::Enabled, Expr::value(enabled))
            .exec(conn)
            .await?;
        Ok(())
    }
}

pub struct RoleService<'a> {
    #[allow(unused)]
    context: &'a Context,
}

impl<'a> RoleService<'a> {
    pub async fn assign_roles<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        user_id: Id,
        role_code: HashSet<&str>,
    ) -> NotifyExchangeResult<usize> {
        let now = Utc::now();
        let user_roles = role_code
            .iter()
            .map(|code| {
                UserRole {
                    id: Id::new(),
                    user_id,
                    role_code: code.to_string(),
                    created_at: now,
                    updated_at: now,
                }
                .into_active_model()
            })
            .collect::<Vec<_>>();

        UserRoleDsl::insert_many(user_roles).exec(conn).await?;

        Ok(role_code.len())
    }

    pub async fn find_roles_by_user<Conn: ConnectionTrait>(
        &self,
        conn: &Conn,
        user_id: Id,
    ) -> NotifyExchangeResult<Vec<Role>> {
        let roles = RoleDsl::find()
            .inner_join(UserRoleDsl)
            .filter(Cond::all().add(UserRoleColumn::UserId.eq(user_id)))
            .all(conn)
            .await?;
        Ok(roles)
    }
}

impl Context {
    pub fn user_service(&self) -> UserService<'_> {
        UserService { context: self }
    }

    pub fn role_service(&self) -> RoleService<'_> {
        RoleService { context: self }
    }
}

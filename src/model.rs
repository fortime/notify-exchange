use std::sync::LazyLock;

use regex::Regex;

pub mod req;

pub mod cache {
    use std::borrow::Cow;

    use sea_orm::prelude::DateTimeUtc;
    use serde::{Deserialize, Serialize};

    use crate::{
        cache::Tokenable,
        db::{
            custom_type::Id,
            entity::{Role, User},
        },
    };

    #[derive(Serialize, Deserialize)]
    pub struct LoginRequestCache {
        pub user_id: Id,
        pub user_email: String,
        /// A code which is set in the cookies and used to verify in `has_login`
        pub code: String,
        pub session_id: Option<String>,
    }

    impl Tokenable for LoginRequestCache {
        fn prefix() -> Cow<'static, str> {
            "login_request".into()
        }
    }

    #[derive(Serialize, Deserialize)]
    pub struct SessionCache {
        pub user: User,
        pub roles: Vec<Role>,
        pub id: String,
        pub csrf_token: String,
        pub updated_at: DateTimeUtc,
        pub expired_at: DateTimeUtc,
    }

    impl Tokenable for SessionCache {
        fn prefix() -> Cow<'static, str> {
            "session".into()
        }
    }

    #[derive(Serialize, Deserialize)]
    pub struct IdCache(pub Id);

    impl Tokenable for IdCache {
        fn prefix() -> Cow<'static, str> {
            "id".into()
        }
    }

    #[derive(Serialize, Deserialize, Clone, Debug)]
    pub struct TelegramSetupCache {
        pub created: bool,
    }

    impl Tokenable for TelegramSetupCache {
        fn prefix() -> Cow<'static, str> {
            "telegram_setup".into()
        }
    }
}

pub static CODE_CHARS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-zA-Z0-9-_]+$").unwrap());

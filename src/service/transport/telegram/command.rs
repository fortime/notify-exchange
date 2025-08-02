use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap},
    marker::PhantomData,
    mem,
    str::FromStr,
    sync::LazyLock,
    time::Duration,
};

use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, IntoActiveModel, sea_query::Cond};
use teloxide::{
    payloads::SetMyCommandsSetters as _,
    prelude::Requester as _,
    types::{
        BotCommand, BotCommandScope, InlineKeyboardButton, InlineKeyboardMarkup, Recipient,
        ReplyMarkup, WebAppInfo,
    },
};
use validator::Validate as _;

use crate::{
    context::Context,
    db::{
        custom_type::{Id, TopicPermissionType},
        entity::{Endpoint, EndpointColumn, PendingUserColumn, Role, SubscriptionColumn, User},
    },
    error::{self, NotifyExchangeError, NotifyExchangeResult},
    model::req::{
        CreateEndpointRequest, CreateHttpTransportServiceRequest, CreateTopicRequest,
        CreateUserRequest,
    },
    service::{transport::http::HttpService, user::PendingUserRequestExt},
    util::{self, ToTuple as _},
};

use super::{ChatContext, TelegramEndpointOptions, TelegramService, user_endpoint_code};

static COMMAND_DICT: LazyLock<BTreeMap<&'static str, Command>> = LazyLock::new(|| {
    let commands: Vec<Command> = vec![
        Command(
            CommandKind::Start,
            MetaCommand {
                name: "/start",
                desc: "Set the menu",
                help: "/start",
                need_admin: false,
                need_registered: false,
                allow_public: true,
                command_fn: StaticErasedCommandFn::wrap(start),
            },
        ),
        Command(
            CommandKind::Help,
            MetaCommand {
                name: "/help",
                desc: "Show this message",
                help: "/help",
                need_admin: false,
                need_registered: false,
                allow_public: true,
                command_fn: StaticErasedCommandFn::wrap(help),
            },
        ),
        Command(
            CommandKind::Register,
            MetaCommand {
                name: "/register",
                desc: "Register as a user",
                help: "/register <email>|<name>",
                need_admin: false,
                need_registered: false,
                allow_public: false,
                command_fn: StaticErasedCommandFn::wrap(register),
            },
        ),
        Command(
            CommandKind::SetAppName,
            MetaCommand {
                name: "/set_app_name",
                desc: "Set the app name of the transport service",
                help: "/set_app_name <app_name>[|[force]]",
                need_admin: true,
                need_registered: true,
                allow_public: false,
                command_fn: StaticErasedCommandFn::wrap(set_app_name),
            },
        ),
        Command(
            CommandKind::StartApp,
            MetaCommand {
                name: "/start_app",
                desc: "Start a mini app",
                help: "/start_app",
                need_admin: false,
                need_registered: true,
                allow_public: false,
                command_fn: StaticErasedCommandFn::wrap(start_app),
            },
        ),
        Command(
            CommandKind::Login,
            MetaCommand {
                name: "/login",
                desc: "Login with the token from web portal",
                help: "/login <token>",
                need_admin: false,
                need_registered: true,
                allow_public: false,
                command_fn: StaticErasedCommandFn::wrap(login),
            },
        ),
        Command(
            CommandKind::SubscribeById,
            MetaCommand {
                name: "/subscribe_by_id",
                desc: "Subscribe to a topic specified by an id",
                help: "/subscribe_by_id <topic_id>",
                need_admin: false,
                need_registered: true,
                allow_public: true,
                command_fn: StaticErasedCommandFn::wrap(subscribe_by_id),
            },
        ),
        Command(
            CommandKind::SubscribeByCode,
            MetaCommand {
                name: "/subscribe_by_code",
                desc: "Subscribe to a topic specified by a code",
                help: "/subscribe_by_code <topic_code>",
                need_admin: false,
                need_registered: true,
                allow_public: true,
                command_fn: StaticErasedCommandFn::wrap(subscribe_by_code),
            },
        ),
        Command(
            CommandKind::PublishById,
            MetaCommand {
                name: "/publish_by_id",
                desc: "Publish a message to a topic specified by an id",
                help: "/publish_by_id <topic_id>|<message>",
                need_admin: false,
                need_registered: true,
                allow_public: false,
                command_fn: StaticErasedCommandFn::wrap(publish_by_id),
            },
        ),
        Command(
            CommandKind::PublishByCode,
            MetaCommand {
                name: "/publish_by_code",
                desc: "Publish a message to a topic specified by a code",
                help: "/publish_by_code <topic_code>|<message>",
                need_admin: false,
                need_registered: true,
                allow_public: false,
                command_fn: StaticErasedCommandFn::wrap(publish_by_code),
            },
        ),
        Command(
            CommandKind::ListTopic,
            MetaCommand {
                name: "/list_topic",
                desc: "Show a list of topics",
                help: "/list_topic [token]",
                need_admin: false,
                need_registered: true,
                allow_public: false,
                command_fn: StaticErasedCommandFn::wrap(list_topic),
            },
        ),
        Command(
            CommandKind::ListSubscription,
            MetaCommand {
                name: "/list_subscription",
                desc: "Show a list of subscription",
                help: "/list_subscription [token]",
                need_admin: false,
                need_registered: true,
                allow_public: true,
                command_fn: StaticErasedCommandFn::wrap(list_subscription),
            },
        ),
        Command(
            CommandKind::ListEndpoint,
            MetaCommand {
                name: "/list_endpoint",
                desc: "Show a list of endpoints",
                help: "/list_endpoint [token]",
                need_admin: false,
                need_registered: true,
                allow_public: false,
                command_fn: StaticErasedCommandFn::wrap(list_endpoint),
            },
        ),
        Command(
            CommandKind::ConfirmUser,
            MetaCommand {
                name: "/confirm_user",
                desc: "Confirm a user request",
                help: "/confirm_user <token>",
                need_admin: true,
                need_registered: true,
                allow_public: false,
                command_fn: StaticErasedCommandFn::wrap(confirm_user),
            },
        ),
        Command(
            CommandKind::CreateTopic,
            MetaCommand {
                name: "/create_topic",
                desc: "Create a topic",
                help: "/create_topic <topic_code>|<topic_name>[|[description]]",
                need_admin: true,
                need_registered: true,
                allow_public: false,
                command_fn: StaticErasedCommandFn::wrap(create_topic),
            },
        ),
        Command(
            CommandKind::CreateHttpService,
            MetaCommand {
                name: "/create_http_service",
                desc: "Create a http transport service",
                help: "/create_http_service <name>[|[description]]",
                need_admin: true,
                need_registered: true,
                allow_public: false,
                command_fn: StaticErasedCommandFn::wrap(create_http_service),
            },
        ),
        Command(
            CommandKind::CreateHttpEndpoint,
            MetaCommand {
                name: "/create_http_endpoint",
                desc: "Create a http transport service endpoint",
                help: "/create_http_endpoint <service_id>|<code>|<name>[|[description]]",
                need_admin: true,
                need_registered: true,
                allow_public: false,
                command_fn: StaticErasedCommandFn::wrap(create_http_endpoint),
            },
        ),
        Command(
            CommandKind::ListEndpointSecret,
            MetaCommand {
                name: "/list_endpoint_secret",
                desc: "List secrets of the endpoint",
                help: "/list_endpoint_secret <endpoin_id>[|[token]]",
                need_admin: false,
                need_registered: true,
                allow_public: false,
                command_fn: StaticErasedCommandFn::wrap(list_endpoint_secret),
            },
        ),
        Command(
            CommandKind::ListAllTopic,
            MetaCommand {
                name: "/list_all_topic",
                desc: "Show all topics",
                help: "/list_all_topic [token]",
                need_admin: true,
                need_registered: true,
                allow_public: false,
                command_fn: StaticErasedCommandFn::wrap(list_all_topic),
            },
        ),
        Command(
            CommandKind::ListPendingUser,
            MetaCommand {
                name: "/list_pending_user",
                desc: "Show a list of pending users",
                help: "/list_pending_user [token]",
                need_admin: true,
                need_registered: true,
                allow_public: false,
                command_fn: StaticErasedCommandFn::wrap(list_pending_user),
            },
        ),
        Command(
            CommandKind::ListUser,
            MetaCommand {
                name: "/list_user",
                desc: "Show a list of users",
                help: "/list_user [token]",
                need_admin: true,
                need_registered: true,
                allow_public: false,
                command_fn: StaticErasedCommandFn::wrap(list_user),
            },
        ),
        Command(
            CommandKind::ListService,
            MetaCommand {
                name: "/list_service",
                desc: "Show a list of transport services",
                help: "/list_service [token]",
                need_admin: true,
                need_registered: true,
                allow_public: false,
                command_fn: StaticErasedCommandFn::wrap(list_service),
            },
        ),
        Command(
            CommandKind::ListServiceEndpoint,
            MetaCommand {
                name: "/list_service_endpoint",
                desc: "Show a list of endpoints of a specific service",
                help: "/list_service_endpoint <service_id>[|[token]]",
                need_admin: true,
                need_registered: true,
                allow_public: false,
                command_fn: StaticErasedCommandFn::wrap(list_service_endpoint),
            },
        ),
        Command(
            CommandKind::ListEndpointSubscription,
            MetaCommand {
                name: "/list_endpoint_subscription",
                desc: "Show a list of subscription of a specific endpoint",
                help: "/list_endpoint_subscription <endpoint_id>[|[token]]",
                need_admin: true,
                need_registered: true,
                allow_public: false,
                command_fn: StaticErasedCommandFn::wrap(list_endpoint_subscription),
            },
        ),
        Command(
            CommandKind::ListTopicSubscription,
            MetaCommand {
                name: "/list_topic_subscription",
                desc: "Show a list of subscription of a specific topic",
                help: "/list_topic_subscription <topic_id>[|[token]]",
                need_admin: true,
                need_registered: true,
                allow_public: false,
                command_fn: StaticErasedCommandFn::wrap(list_topic_subscription),
            },
        ),
        Command(
            CommandKind::GetUser,
            MetaCommand {
                name: "/get_user",
                desc: "Show user info by id",
                help: "/get_user <user_id>",
                need_admin: true,
                need_registered: true,
                allow_public: false,
                command_fn: StaticErasedCommandFn::wrap(get_user),
            },
        ),
        Command(
            CommandKind::GetTopic,
            MetaCommand {
                name: "/get_topic",
                desc: "Show topic info by id",
                help: "/get_topic <topic_id>",
                need_admin: true,
                need_registered: true,
                allow_public: false,
                command_fn: StaticErasedCommandFn::wrap(get_topic),
            },
        ),
        Command(
            CommandKind::GetSubscription,
            MetaCommand {
                name: "/get_subscription",
                desc: "Show subscription info by id",
                help: "/get_subscription <subscription_id>",
                need_admin: true,
                need_registered: true,
                allow_public: false,
                command_fn: StaticErasedCommandFn::wrap(get_subscription),
            },
        ),
        Command(
            CommandKind::DeleteExpiredPendingUser,
            MetaCommand {
                name: "/delete_expired_pending_user",
                desc: "Delete at most n expired pending users, n should be less than 30. Deleted records will be returned",
                help: "/delete_expired_pending_user [num]",
                need_admin: true,
                need_registered: true,
                allow_public: false,
                command_fn: StaticErasedCommandFn::wrap(delete_expired_pending_user),
            },
        ),
        Command(
            CommandKind::DeleteAllExpiredPendingUser,
            MetaCommand {
                name: "/delete_all_expired_pending_user",
                desc: "Delete all expired pending users",
                help: "/delete_all_expired_pending_user",
                need_admin: true,
                need_registered: true,
                allow_public: false,
                command_fn: StaticErasedCommandFn::wrap(delete_all_expired_pending_user),
            },
        ),
        Command(
            CommandKind::SetUserEnabled,
            MetaCommand {
                name: "/set_user_enabled",
                desc: "Set the enabled field of the user",
                help: "/set_user_enabled <user_id>|<enabled>",
                need_admin: true,
                need_registered: true,
                allow_public: false,
                command_fn: StaticErasedCommandFn::wrap(set_user_enabled),
            },
        ),
    ];

    let mut map = BTreeMap::new();
    for command in commands {
        map.insert(command.1.name, command);
    }
    map
});

macro_rules! impl_command_fn {
    ($($ty: tt),* $(,)?) => {
        #[async_trait::async_trait]
        impl<'a, F, Fut$(, $ty)*> CommandFn<'a, (F, Fut$(, $ty)*)> for F
        where
            F: (Fn($($ty,)*) -> Fut) + Send + Sync,
            Fut: 'a + Future<Output=NotifyExchangeResult<()>> + Send,
            $( $ty: TryFrom<&'a CommandContext<'a>, Error=NotifyExchangeError>, )*
        {
            #[allow(non_snake_case, unused)]
            async fn execute(&self, command_ctx: &'a CommandContext<'a>) -> NotifyExchangeResult<()> {
                $(
                    let $ty = $ty::try_from(command_ctx)?;
                )*

                self($($ty,)*).await
            }
        }
    };
}

/// This follows `axum::Handler`'s implementation.
#[async_trait::async_trait]
trait CommandFn<'a, T> {
    async fn execute(&self, command_ctx: &'a CommandContext<'a>) -> NotifyExchangeResult<()>;
}

struct CommandFnContainer<'a, CF, T> {
    cf: CF,
    phantom: PhantomData<&'a T>,
}

// Safety, it will only be used as the container of `async fn`
unsafe impl<'a, CF, T> Send for CommandFnContainer<'a, CF, T> where CF: CommandFn<'a, T> {}
unsafe impl<'a, CF, T> Sync for CommandFnContainer<'a, CF, T> where CF: CommandFn<'a, T> {}

impl<'a, CF, T> CommandFnContainer<'a, CF, T>
where
    CF: 'a + CommandFn<'a, T> + Send + Sync,
    T: 'a + Send,
{
    fn wrap(cf: CF) -> Box<dyn ErasedCommandFn<'a> + Send + Sync + 'a> {
        Box::new(Self {
            cf,
            phantom: Default::default(),
        })
    }
}

#[async_trait::async_trait]
trait ErasedCommandFn<'a> {
    async fn execute(&self, command_ctx: &'a CommandContext<'a>) -> NotifyExchangeResult<()>;
}

#[async_trait::async_trait]
impl<'a, CF, T> ErasedCommandFn<'a> for CommandFnContainer<'a, CF, T>
where
    CF: 'a + CommandFn<'a, T> + Send + Sync,
{
    async fn execute(&self, command_ctx: &'a CommandContext<'a>) -> NotifyExchangeResult<()> {
        self.cf.execute(command_ctx).await
    }
}

struct StaticErasedCommandFn(Box<dyn ErasedCommandFn<'static> + Send + Sync>);

impl StaticErasedCommandFn {
    fn wrap<'a, CF, T>(cf: CF) -> Self
    where
        CF: 'a + CommandFn<'a, T> + Send + Sync,
        T: 'a + Send,
    {
        let c = CommandFnContainer::wrap(cf);
        // Safety, `async fn` can be treated as `'static`.
        Self(unsafe {
            mem::transmute::<
                Box<dyn ErasedCommandFn<'a> + Send + Sync + 'a>,
                Box<dyn ErasedCommandFn<'static> + Send + Sync>,
            >(c)
        })
    }

    async fn execute<'a>(&self, command_ctx: &'a CommandContext<'a>) -> NotifyExchangeResult<()> {
        // Safety, `async fn` can be treated as `'static`, here, we downcast the lifetime to meet
        // the trait requirement.
        unsafe {
            mem::transmute::<
                &Box<dyn ErasedCommandFn<'static> + Send + Sync>,
                &Box<dyn ErasedCommandFn<'a> + Send + Sync + 'a>,
            >(&self.0)
        }
        .execute(command_ctx)
        .await
    }
}

impl_command_fn!();
impl_command_fn!(T1);
impl_command_fn!(T1, T2);
impl_command_fn!(T1, T2, T3);
impl_command_fn!(T1, T2, T3, T4);
impl_command_fn!(T1, T2, T3, T4, T5);
impl_command_fn!(T1, T2, T3, T4, T5, T6);
impl_command_fn!(T1, T2, T3, T4, T5, T6, T7);
impl_command_fn!(T1, T2, T3, T4, T5, T6, T7, T8);
impl_command_fn!(T1, T2, T3, T4, T5, T6, T7, T8, T9);

struct CommandContext<'a> {
    command: &'a Command,
    context: &'a Context,
    telegram_service: &'a TelegramService<'a>,
    chat_ctx: &'a ChatContext<'a>,
    params: &'a str,
}

impl<'a> TryFrom<&'a CommandContext<'a>> for &'a Context {
    type Error = NotifyExchangeError;

    fn try_from(command_ctx: &'a CommandContext<'a>) -> Result<Self, Self::Error> {
        Ok(command_ctx.context)
    }
}

impl<'a> TryFrom<&'a CommandContext<'a>> for &'a TelegramService<'a> {
    type Error = NotifyExchangeError;

    fn try_from(command_ctx: &'a CommandContext<'a>) -> Result<Self, Self::Error> {
        Ok(command_ctx.telegram_service)
    }
}

impl<'a> TryFrom<&'a CommandContext<'a>> for &'a MetaCommand {
    type Error = NotifyExchangeError;

    fn try_from(command_ctx: &'a CommandContext<'a>) -> Result<Self, Self::Error> {
        Ok(&command_ctx.command.1)
    }
}

impl<'a> TryFrom<&'a CommandContext<'a>> for CommandKind {
    type Error = NotifyExchangeError;

    fn try_from(command_ctx: &'a CommandContext<'a>) -> Result<Self, Self::Error> {
        Ok(command_ctx.command.0)
    }
}

impl<'a> TryFrom<&'a CommandContext<'a>> for &'a ChatContext<'a> {
    type Error = NotifyExchangeError;

    fn try_from(command_ctx: &'a CommandContext<'a>) -> Result<Self, Self::Error> {
        Ok(command_ctx.chat_ctx)
    }
}

impl<'a> TryFrom<&'a CommandContext<'a>> for &'a User {
    type Error = NotifyExchangeError;

    fn try_from(command_ctx: &'a CommandContext<'a>) -> Result<Self, Self::Error> {
        let Some((user, _)) = command_ctx.chat_ctx.user_and_endpoint else {
            tracing::info!(
                "A unregistered user calls a command[{}]",
                command_ctx.command.1.name
            );
            return Err(error::invalid_request("Unknown command"));
        };
        Ok(user)
    }
}

impl<'a> TryFrom<&'a CommandContext<'a>> for &'a Endpoint {
    type Error = NotifyExchangeError;

    fn try_from(command_ctx: &'a CommandContext<'a>) -> Result<Self, Self::Error> {
        let Some((_, endpoint)) = command_ctx.chat_ctx.user_and_endpoint else {
            tracing::info!(
                "A unregistered user calls a command[{}]",
                command_ctx.command.1.name
            );
            return Err(error::invalid_request("Unknown command"));
        };
        Ok(endpoint)
    }
}

impl<'a> TryFrom<&'a CommandContext<'a>> for Option<&'a User> {
    type Error = NotifyExchangeError;

    fn try_from(command_ctx: &'a CommandContext<'a>) -> Result<Self, Self::Error> {
        Ok(command_ctx.chat_ctx.user_and_endpoint.map(|(u, _)| u))
    }
}

impl<'a> TryFrom<&'a CommandContext<'a>> for Option<&'a Endpoint> {
    type Error = NotifyExchangeError;

    fn try_from(command_ctx: &'a CommandContext<'a>) -> Result<Self, Self::Error> {
        Ok(command_ctx.chat_ctx.user_and_endpoint.map(|(_, e)| e))
    }
}

struct Params<T>(T);

impl<'a> TryFrom<&'a CommandContext<'a>> for Params<Option<&'a str>> {
    type Error = NotifyExchangeError;

    fn try_from(command_ctx: &'a CommandContext<'a>) -> Result<Self, Self::Error> {
        let params = command_ctx.params.trim();
        let params = if params.is_empty() {
            None
        } else {
            Some(params)
        };
        Ok(Params(params))
    }
}

impl<'a> TryFrom<&'a CommandContext<'a>> for Params<&'a str> {
    type Error = NotifyExchangeError;

    fn try_from(command_ctx: &'a CommandContext<'a>) -> Result<Self, Self::Error> {
        let params = command_ctx.params.trim();
        if params.is_empty() {
            return Err(error::invalid_request(format!(
                "Invalid command, it should be called like: {}",
                command_ctx.command.1.help
            )));
        }
        Ok(Params(params))
    }
}

impl<'a> TryFrom<&'a CommandContext<'a>> for Params<(&'a str, &'a str)> {
    type Error = NotifyExchangeError;

    fn try_from(command_ctx: &'a CommandContext<'a>) -> Result<Self, Self::Error> {
        let (arg1, arg2) =
            util::extract_params::<2>(command_ctx.params, command_ctx.command.1.help)?.to_tuple();
        Ok(Params((arg1, arg2)))
    }
}

impl<'a> TryFrom<&'a CommandContext<'a>> for Params<(Id, bool)> {
    type Error = NotifyExchangeError;

    fn try_from(command_ctx: &'a CommandContext<'a>) -> Result<Self, Self::Error> {
        let (arg1, arg2): (&str, &str) =
            util::extract_params::<2>(command_ctx.params, command_ctx.command.1.help)?.to_tuple();
        Ok(Params((
            arg1.parse()?,
            arg2.parse()
                .map_err(|_| error::invalid_request("Invalid boolean"))?,
        )))
    }
}

impl<'a> TryFrom<&'a CommandContext<'a>> for Params<(&'a str, Option<&'a str>)> {
    type Error = NotifyExchangeError;

    fn try_from(command_ctx: &'a CommandContext<'a>) -> Result<Self, Self::Error> {
        let (arg1, arg2) =
            match util::extract_params::<2>(command_ctx.params, command_ctx.command.1.help) {
                Ok(args) => {
                    let args = args.to_tuple();
                    (args.0, Some(args.1))
                }
                Err(e) => {
                    if e.is_invalid_request() {
                        let params = command_ctx.params.trim();
                        if params.is_empty() {
                            return Err(error::invalid_request(format!(
                                "Invalid command, it should be called like: {}",
                                command_ctx.command.1.help
                            )));
                        }
                        (params, None)
                    } else {
                        return Err(e);
                    }
                }
            };
        Ok(Params((arg1, arg2)))
    }
}

impl<'a> TryFrom<&'a CommandContext<'a>> for Params<(&'a str, &'a str, Option<&'a str>)> {
    type Error = NotifyExchangeError;

    fn try_from(command_ctx: &'a CommandContext<'a>) -> Result<Self, Self::Error> {
        let (arg1, arg2, arg3) =
            match util::extract_params::<3>(command_ctx.params, command_ctx.command.1.help) {
                Ok(args) => {
                    let args = args.to_tuple();
                    (args.0, args.1, Some(args.2))
                }
                Err(e) => {
                    if e.is_invalid_request() {
                        let (arg0, arg1) = util::extract_params::<2>(
                            command_ctx.params,
                            command_ctx.command.1.help,
                        )?
                        .to_tuple();
                        (arg0, arg1, None)
                    } else {
                        return Err(e);
                    }
                }
            };
        Ok(Params((arg1, arg2, arg3)))
    }
}

impl<'a> TryFrom<&'a CommandContext<'a>> for Params<(&'a str, &'a str, &'a str)> {
    type Error = NotifyExchangeError;

    fn try_from(command_ctx: &'a CommandContext<'a>) -> Result<Self, Self::Error> {
        let (arg1, arg2, arg3) =
            util::extract_params::<3>(command_ctx.params, command_ctx.command.1.help)?.to_tuple();
        Ok(Params((arg1, arg2, arg3)))
    }
}

impl<'a> TryFrom<&'a CommandContext<'a>> for Params<(&'a str, &'a str, &'a str, Option<&'a str>)> {
    type Error = NotifyExchangeError;

    fn try_from(command_ctx: &'a CommandContext<'a>) -> Result<Self, Self::Error> {
        let (arg1, arg2, arg3, arg4) =
            match util::extract_params::<4>(command_ctx.params, command_ctx.command.1.help) {
                Ok(args) => {
                    let args = args.to_tuple();
                    (args.0, args.1, args.2, Some(args.3))
                }
                Err(e) => {
                    if e.is_invalid_request() {
                        let (arg0, arg1, arg2) = util::extract_params::<3>(
                            command_ctx.params,
                            command_ctx.command.1.help,
                        )?
                        .to_tuple();
                        (arg0, arg1, arg2, None)
                    } else {
                        return Err(e);
                    }
                }
            };
        Ok(Params((arg1, arg2, arg3, arg4)))
    }
}

pub struct MetaCommand {
    pub name: &'static str,
    pub desc: &'static str,
    pub help: &'static str,
    pub need_admin: bool,
    pub need_registered: bool,
    pub allow_public: bool,
    command_fn: StaticErasedCommandFn,
}

#[derive(Clone, Copy)]
pub enum CommandKind {
    Start,
    Help,
    Register,
    SetAppName,
    StartApp,
    Login,
    SubscribeById,
    SubscribeByCode,
    PublishById,
    PublishByCode,
    ListTopic,
    ListSubscription,
    ListEndpoint,
    ConfirmUser,
    CreateTopic,
    CreateHttpService,
    CreateHttpEndpoint,
    ListEndpointSecret,
    ListAllTopic,
    ListPendingUser,
    ListUser,
    ListService,
    ListServiceEndpoint,
    ListEndpointSubscription,
    ListTopicSubscription,
    GetUser,
    GetTopic,
    GetSubscription,
    DeleteExpiredPendingUser,
    DeleteAllExpiredPendingUser,
    SetUserEnabled,
}

pub struct Command(CommandKind, MetaCommand);

impl Command {
    pub fn kind(&self) -> &CommandKind {
        &self.0
    }

    pub async fn execute(
        &self,
        context: &Context,
        telegram_service: &TelegramService<'_>,
        chat_ctx: &ChatContext<'_>,
        params: &str,
    ) -> NotifyExchangeResult<()> {
        if chat_ctx.is_public() && !self.1.allow_public {
            tracing::info!(
                "Command[{}] is called from a group but it is not allowed",
                self.1.name
            );
            return Err(error::invalid_request("Unknown command"));
        }
        if self.1.need_admin && !chat_ctx.is_admin {
            tracing::info!("A normal user calls an admin command[{}]", self.1.name);
            return Err(error::invalid_request("Unknown command"));
        }
        self.1
            .command_fn
            .execute(&CommandContext {
                command: self,
                context,
                telegram_service,
                chat_ctx,
                params,
            })
            .await
    }

    pub fn executable_command(chat_ctx: &ChatContext<'_>) -> Vec<&'static Command> {
        let mut res = vec![];

        for command in COMMAND_DICT.values() {
            if chat_ctx.is_public() && !command.1.allow_public {
                continue;
            }
            if chat_ctx.user_and_endpoint.is_none() && command.1.need_registered {
                continue;
            }
            if !chat_ctx.is_admin && command.1.need_admin {
                continue;
            }
            res.push(command);
        }

        res
    }

    pub fn to_bot_command(&self) -> BotCommand {
        BotCommand::new(&self.1.name[1..], self.1.desc)
    }
}

impl FromStr for &Command {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        COMMAND_DICT.get(s).ok_or(())
    }
}

/// A holder fn for command to remind me something should be done.
#[allow(unused)]
#[deprecated]
async fn todo_holder() -> NotifyExchangeResult<()> {
    Ok(())
}

async fn start(chat_ctx: &ChatContext<'_>) -> NotifyExchangeResult<()> {
    let scope = if chat_ctx.is_public() {
        BotCommandScope::ChatMember {
            chat_id: Recipient::Id(chat_ctx.chat.id),
            user_id: chat_ctx.sender.id,
        }
    } else {
        BotCommandScope::Chat {
            chat_id: Recipient::Id(chat_ctx.chat.id),
        }
    };
    chat_ctx
        .bot
        .set_my_commands(
            Command::executable_command(chat_ctx)
                .into_iter()
                .map(Command::to_bot_command),
        )
        .scope(scope)
        .await?;
    Ok(())
}

async fn help(chat_ctx: &ChatContext<'_>) -> NotifyExchangeResult<()> {
    let mut message = "Help:\n".to_string();
    for command in Command::executable_command(chat_ctx) {
        message.push_str(command.1.name);
        message.push_str(" // ");
        message.push_str(command.1.desc);
        message.push('\n');
    }
    chat_ctx.send_message(message).await?;
    Ok(())
}

async fn register(
    context: &Context,
    chat_ctx: &ChatContext<'_>,
    Params((email, name)): Params<(&str, &str)>,
) -> NotifyExchangeResult<()> {
    if chat_ctx.user_and_endpoint.is_some() {
        chat_ctx.send_message("You already registered").await?;
        return Ok(());
    }

    let req = CreateUserRequest {
        name: name.into(),
        email: email.into(),
    };
    req.validate()?;

    let ext = PendingUserRequestExt {
        roles: vec![Role::USER.to_string()],
        endpoints: vec![CreateEndpointRequest {
            code: Cow::Owned(user_endpoint_code(chat_ctx.sender.id)),
            name: Cow::Owned(chat_ctx.sender.full_name()),
            transport_service_id: chat_ctx.transport_service.id,
            transport_service_type: chat_ctx.transport_service.transport_service_type,
            description: None,
            options: TelegramService::serialize_endpoint_options(&TelegramEndpointOptions {
                chat_id: chat_ctx.chat.id,
            })?,
            is_public: false,
        }],
    };

    let pending_user = context
        .user_service()
        .create_pending_user(context.db(), req, ext, Duration::from_secs(7 * 24 * 3600))
        .await?;

    chat_ctx
        .send_message(format!(
            "Ask any admin to confirm the request before {}, token: {}",
            pending_user.expired_at, pending_user.id
        ))
        .await?;
    Ok(())
}

async fn set_app_name(
    meta_command: &MetaCommand,
    context: &Context,
    chat_ctx: &ChatContext<'_>,
    Params((app_name, force)): Params<(&str, Option<&str>)>,
) -> NotifyExchangeResult<()> {
    let force = force.and_then(|s| s.parse().ok()).unwrap_or(false);
    let mut options =
        TelegramService::extract_transport_service_options(&chat_ctx.transport_service.options)?;
    let mut old = None;
    if let Some(old_app_name) = &mut options.app_name {
        if old_app_name == app_name {
            return chat_ctx.send_message("App name is the same").await;
        }
        if !force {
            return chat_ctx
                .send_message(format!(
                    "App name is set. If you really want to change it, send: `{} {}|true`",
                    meta_command.name, app_name
                ))
                .await;
        }
        let mut tmp = app_name.to_string();
        mem::swap(&mut tmp, old_app_name);
        old = Some(tmp);
    } else {
        options.app_name = Some(app_name.to_string());
    }

    let mut transport_service = chat_ctx.transport_service.clone().into_active_model();
    transport_service.options.set_if_not_equals(
        TelegramService::serialize_transport_service_options(&options)?,
    );
    transport_service.update(context.db()).await?;

    if let Some(old) = old {
        chat_ctx
            .send_message(format!("App name is change from {old} to {app_name}"))
            .await
    } else {
        chat_ctx
            .send_message(format!("App name is set to {app_name}"))
            .await
    }
}

async fn start_app(
    telegram_service: &TelegramService<'_>,
    chat_ctx: &ChatContext<'_>,
    endpoint: &Endpoint,
) -> NotifyExchangeResult<()> {
    let app_url = telegram_service.start_app_with_url(endpoint, None).await?;
    let inline_keyboard =
        InlineKeyboardMarkup::default().append_row([InlineKeyboardButton::web_app(
            "Open App",
            WebAppInfo { url: app_url },
        )]);
    chat_ctx
        .send_message_with_reply_markup("Welcome", ReplyMarkup::InlineKeyboard(inline_keyboard))
        .await?;
    Ok(())
}

async fn login(
    context: &Context,
    chat_ctx: &ChatContext<'_>,
    user: &User,
    Params(token): Params<&str>,
) -> NotifyExchangeResult<()> {
    if let Err(e) = context
        .user_service()
        .login_with_token(context.db(), Cow::Borrowed(user), token)
        .await
    {
        tracing::debug!("Login error: {e:?}");
        chat_ctx.send_message("Unable to login").await?;
    } else {
        chat_ctx.send_message("You're in. Welcome!").await?;
    };
    Ok(())
}

async fn subscribe_by_id(
    context: &Context,
    telegram_service: &TelegramService<'_>,
    chat_ctx: &ChatContext<'_>,
    user: &User,
    Params(topic_id): Params<&str>,
) -> NotifyExchangeResult<()> {
    let topic_id: Id = topic_id
        .parse()
        .map_err(|_| error::invalid_request("Invalid Id"))?;
    let subscription = context
        .transaction(async |conn| {
            let endpoint = telegram_service
                .find_or_create_chat_endpoint(conn, chat_ctx, user)
                .await?;
            context
                .topic_service()
                .subscribe(conn, topic_id, &endpoint, user.id)
                .await
        })
        .await?;
    chat_ctx
        .send_message(format!(
            "You have subscribed to the topic with id: {}, subscription id: {}",
            topic_id, subscription.id
        ))
        .await?;
    Ok(())
}

async fn subscribe_by_code(
    context: &Context,
    telegram_service: &TelegramService<'_>,
    chat_ctx: &ChatContext<'_>,
    user: &User,
    Params(topic_code): Params<&str>,
) -> NotifyExchangeResult<()> {
    let subscription = context
        .transaction(async |conn| {
            let endpoint = telegram_service
                .find_or_create_chat_endpoint(conn, chat_ctx, user)
                .await?;
            context
                .topic_service()
                .subscribe_by_uniq_key(conn, topic_code, None, &endpoint, user.id)
                .await
        })
        .await?;
    chat_ctx
        .send_message(format!(
            "You have subscribed to the topic with code: {}, subscription id: {}",
            topic_code, subscription.id
        ))
        .await?;
    Ok(())
}

async fn publish_by_id(
    context: &Context,
    chat_ctx: &ChatContext<'_>,
    user: &User,
    endpoint: &Endpoint,
    Params((topic_id, message)): Params<(&str, &str)>,
) -> NotifyExchangeResult<()> {
    let topic_id: Id = topic_id
        .parse()
        .map_err(|_| error::invalid_request("Invalid Id"))?;
    let message_id = context
        .topic_service()
        .publish(
            context.db(),
            topic_id,
            endpoint.id,
            user.id,
            message.as_bytes(),
        )
        .await?;
    chat_ctx
        .send_message(format!("Message published, id: {}", message_id))
        .await?;
    Ok(())
}

async fn publish_by_code(
    context: &Context,
    chat_ctx: &ChatContext<'_>,
    user: &User,
    endpoint: &Endpoint,
    Params((topic_code, message)): Params<(&str, &str)>,
) -> NotifyExchangeResult<()> {
    let message_id = context
        .topic_service()
        .publish_by_uniq_key(
            context.db(),
            topic_code,
            None,
            endpoint.id,
            user.id,
            message.as_bytes(),
        )
        .await?;
    chat_ctx
        .send_message(format!("Message published, id: {}", message_id))
        .await?;
    Ok(())
}

async fn list_topic(
    context: &Context,
    telegram_service: &TelegramService<'_>,
    chat_ctx: &ChatContext<'_>,
    user: &User,
    Params(token): Params<Option<&str>>,
) -> NotifyExchangeResult<()> {
    let last_topic_id = if let Some(token) = token {
        Some(telegram_service.get_id_by_token(token).await?)
    } else {
        None
    };

    let (topics, last_topic_id) = context
        .topic_service()
        .list_authorized_topic(context.db(), user.id, last_topic_id, TelegramService::LIMIT)
        .await?;

    let mut message = "Topics:\n".to_string();
    for (topic, permissions) in topics {
        let permissions = permissions
            .iter()
            .map(TopicPermissionType::to_short_str)
            .collect::<Vec<_>>()
            .join(",");
        message.push_str(&format!(
            "{}|{}|{}|{}: [{}]\n",
            topic.id, topic.name, topic.code, topic.user_id, permissions
        ));
    }

    if let Some(last_topic_id) = last_topic_id {
        let token = telegram_service.add_id_token(last_topic_id).await?;
        message.push_str(&format!("----\nQuery more by token: {token}"));
    } else {
        message.push_str("----\nEnd");
    }

    chat_ctx.send_message(message).await?;
    Ok(())
}

async fn list_subscription_base(
    context: &Context,
    telegram_service: &TelegramService<'_>,
    chat_ctx: &ChatContext<'_>,
    cond: Cond,
    token: Option<&str>,
) -> NotifyExchangeResult<()> {
    let last_subscription_id = if let Some(token) = token {
        Some(telegram_service.get_id_by_token(token).await?)
    } else {
        None
    };

    let (subscriptions, last_subscription_id) = context
        .subscription_service()
        .list_subscription(
            context.db(),
            cond,
            last_subscription_id,
            TelegramService::LIMIT,
        )
        .await?;

    let topic_ids: Vec<_> = subscriptions.iter().map(|s| s.0.topic_id).collect();
    let topics: HashMap<_, _> = context
        .topic_service()
        .find_topics_with_latest_message_id(context.db(), topic_ids)
        .await?
        .into_iter()
        .map(|(t, m)| (t.id, (t, m)))
        .collect();

    let mut message = "Subscriptions:\n".to_string();

    for (subscription, offset) in subscriptions {
        let Some((topic, latest_message_id)) = topics.get(&subscription.topic_id) else {
            return Err(error::internal_server_error(format!(
                "Topic[{}] not exist",
                subscription.topic_id
            )));
        };
        let endpoint = context
            .endpoint_service()
            .find_by_id(context.db(), subscription.endpoint_id)
            .await?;
        let Some(offset) = offset else {
            return Err(error::internal_server_error(format!(
                "Subscription[{}] without offset record",
                subscription.id
            )));
        };
        message.push_str(&format!(
            "{}|{}|{}|{}|[{}]|offset[{}/{}]\n",
            subscription.id,
            topic.id,
            topic.code,
            topic.name,
            endpoint
                .as_ref()
                .map(|e| e.name.as_str())
                .unwrap_or("No endpoint"),
            offset.next_message_id - 1,
            latest_message_id
        ));
    }

    if let Some(last_subscription_id) = last_subscription_id {
        let token = telegram_service.add_id_token(last_subscription_id).await?;
        message.push_str(&format!("----\nQuery more by token: {token}"));
    } else {
        message.push_str("----\nEnd");
    }

    chat_ctx.send_message(message).await?;
    Ok(())
}

async fn list_subscription(
    context: &Context,
    telegram_service: &TelegramService<'_>,
    chat_ctx: &ChatContext<'_>,
    user: &User,
    Params(token): Params<Option<&str>>,
) -> NotifyExchangeResult<()> {
    list_subscription_base(
        context,
        telegram_service,
        chat_ctx,
        Cond::all().add(SubscriptionColumn::UserId.eq(user.id)),
        token,
    )
    .await
}

async fn list_endpoint(
    context: &Context,
    telegram_service: &TelegramService<'_>,
    chat_ctx: &ChatContext<'_>,
    user: &User,
    Params(token): Params<Option<&str>>,
) -> NotifyExchangeResult<()> {
    let last_endpoint_id = if let Some(token) = token {
        Some(telegram_service.get_id_by_token(token).await?)
    } else {
        None
    };

    let (endpoints, last_endpoint_id) = context
        .endpoint_service()
        .list_endpoint(
            context.db(),
            Cond::all().add(EndpointColumn::UserId.eq(user.id)),
            last_endpoint_id,
            TelegramService::LIMIT,
        )
        .await?;

    let mut message = "Endpoints:\n".to_string();

    for endpoint in endpoints {
        message.push_str(&format!(
            "{}|{}|{}|{:?}|{}\n",
            endpoint.id,
            endpoint.code,
            endpoint.name,
            endpoint.transport_service_type,
            endpoint.transport_service_id
        ));
    }

    if let Some(last_endpoint_id) = last_endpoint_id {
        let token = telegram_service.add_id_token(last_endpoint_id).await?;
        message.push_str(&format!("----\nQuery more by token: {token}"));
    } else {
        message.push_str("----\nEnd");
    }

    chat_ctx.send_message(message).await?;
    Ok(())
}

async fn confirm_user(
    context: &Context,
    chat_ctx: &ChatContext<'_>,
    admin: &User,
    Params(token): Params<&str>,
) -> NotifyExchangeResult<()> {
    let pending_user_id = token.parse()?;

    let user = context
        .transaction(async |conn| {
            let (user, ext) = match context
                .user_service()
                .confirm_pending_user(conn, pending_user_id)
                .await
            {
                Ok(v) => v,
                // Reply with the error message
                Err(NotifyExchangeError::NotFound { message }) => {
                    return Err(error::invalid_request(message));
                }
                Err(e) => {
                    return Err(e);
                }
            };
            context
                .endpoint_service()
                .insert_endpoints(conn, user.id, ext.endpoints)
                .await?;
            Ok(user)
        })
        .await?;

    tracing::info!(
        "User[{}/{}] is created with token[{}] by admin[{}/{}]",
        user.id,
        user.email,
        pending_user_id,
        admin.id,
        admin.email,
    );

    chat_ctx
        .send_message(format!(
            "User [{}/{}] has been confirmed",
            user.id, user.email
        ))
        .await?;
    Ok(())
}

async fn create_topic(
    context: &Context,
    chat_ctx: &ChatContext<'_>,
    user: &User,
    Params((code, name, description)): Params<(&str, &str, Option<&str>)>,
) -> NotifyExchangeResult<()> {
    let topic = context
        .transaction(async |conn| {
            context
                .topic_service()
                .create_topic(
                    conn,
                    user.id,
                    CreateTopicRequest {
                        name: name.into(),
                        code: code.into(),
                        description: description.map(Cow::Borrowed),
                    },
                )
                .await
        })
        .await?;

    chat_ctx
        .send_message(format!(
            "The topic [{}/{}] has been created, its id is: {}",
            topic.code, topic.name, topic.id
        ))
        .await?;
    Ok(())
}

async fn create_http_service(
    context: &Context,
    chat_ctx: &ChatContext<'_>,
    user: &User,
    Params((name, description)): Params<(&str, Option<&str>)>,
) -> NotifyExchangeResult<()> {
    let http_service = HttpService::new(context);

    let service = context
        .transaction(async |conn| {
            http_service
                .create_transport_service(
                    conn,
                    user,
                    CreateHttpTransportServiceRequest {
                        name: name.into(),
                        description: description.map(Cow::Borrowed),
                    },
                )
                .await
        })
        .await?;

    let mut message = "Http Service:\n".to_string();
    message.push_str(&format!(
        "{}|{:?}|{}|{}: [{}]\n",
        service.name,
        service.transport_service_type,
        service.created_at,
        service.description.unwrap_or_default(),
        service.id,
    ));
    chat_ctx.send_message(message).await?;
    Ok(())
}

async fn create_http_endpoint(
    context: &Context,
    telegram_service: &TelegramService<'_>,
    chat_ctx: &ChatContext<'_>,
    user: &User,
    user_endpoint: &Endpoint,
    Params((service_id, code, name, description)): Params<(&str, &str, &str, Option<&str>)>,
) -> NotifyExchangeResult<()> {
    let service_id: Id = service_id
        .parse()
        .map_err(|_| error::invalid_request("Invalid Id"))?;

    let endpoint = context
        .transaction(async |conn| {
            HttpService::new(context)
                .create_endpoint(conn, user, service_id, code, name, description)
                .await
        })
        .await?;
    let mut message = "Endpoints:\n".to_string();

    message.push_str(&format!(
        "{}|{}|{}|{:?}|{}\n",
        endpoint.id,
        endpoint.code,
        endpoint.name,
        endpoint.transport_service_type,
        endpoint.transport_service_id,
    ));
    message.push_str("----\nEnd");

    let mut path = context
        .config()
        .web_paths()
        .endpoint_path()
        .replace("{endpoint_id}", &endpoint.id.to_string());
    if !path.starts_with("/") {
        path.insert(0, '/');
    }
    let endpoint_url = telegram_service
        .start_app_with_url(user_endpoint, Some(path))
        .await?;
    let inline_keyboard =
        InlineKeyboardMarkup::default().append_row([InlineKeyboardButton::web_app(
            "Manage Endpoint Secret",
            WebAppInfo { url: endpoint_url },
        )]);
    chat_ctx
        .send_message_with_reply_markup(message, ReplyMarkup::InlineKeyboard(inline_keyboard))
        .await?;
    Ok(())
}

async fn list_endpoint_secret(
    context: &Context,
    telegram_service: &TelegramService<'_>,
    chat_ctx: &ChatContext<'_>,
    user: &User,
    Params((endpoint_id, token)): Params<(&str, Option<&str>)>,
) -> NotifyExchangeResult<()> {
    let endpoint_id: Id = endpoint_id
        .parse()
        .map_err(|_| error::invalid_request("Invalid Id"))?;
    let last_endpoint_secret_id = if let Some(token) = token {
        Some(telegram_service.get_id_by_token(token).await?)
    } else {
        None
    };

    let mut allow = true;
    if !chat_ctx.is_admin {
        // if the user is not admin, show the endpoint of the user only.
        let endpoint = context
            .endpoint_service()
            .find_by_id(context.db(), endpoint_id)
            .await?;
        allow = endpoint.filter(|e| e.user_id == user.id).is_some();
    }

    let (endpoint_secrets, last_endpoint_secret_id) = if allow {
        context
            .endpoint_service()
            .list_endpoint_secret(
                context.db(),
                endpoint_id,
                Cond::all(),
                last_endpoint_secret_id,
                TelegramService::LIMIT,
            )
            .await?
    } else {
        (vec![], None)
    };

    let mut message = "Endpoint Secrets:\n".to_string();

    for endpoint_secret in endpoint_secrets {
        message.push_str(&format!(
            "{}|{}|{}|{}|{}|{:?}|{}\n",
            endpoint_secret.id,
            endpoint_secret.code,
            endpoint_secret.version,
            endpoint_secret.secret_type,
            endpoint_secret.created_at,
            endpoint_secret.expired_at,
            endpoint_secret.enabled,
        ));
    }

    if let Some(last_endpoint_secret_id) = last_endpoint_secret_id {
        let token = telegram_service
            .add_id_token(last_endpoint_secret_id)
            .await?;
        message.push_str(&format!("----\nQuery more by token: {token}"));
    } else {
        message.push_str("----\nEnd");
    }

    chat_ctx.send_message(message).await?;
    Ok(())
}

async fn list_all_topic(
    context: &Context,
    telegram_service: &TelegramService<'_>,
    chat_ctx: &ChatContext<'_>,
    user: &User,
    Params(token): Params<Option<&str>>,
) -> NotifyExchangeResult<()> {
    let last_topic_id = if let Some(token) = token {
        Some(telegram_service.get_id_by_token(token).await?)
    } else {
        None
    };

    let (topics, last_topic_id) = context
        .topic_service()
        .list_topic(context.db(), last_topic_id, TelegramService::LIMIT)
        .await?;

    let mut message = "Topics:\n".to_string();
    for topic in topics {
        let permissions = context
            .topic_service()
            .find_user_topic_permissions(context.db(), topic.id, user.id)
            .await?
            .into_iter()
            .map(|tp| tp.permission_type.to_short_str())
            .collect::<Vec<_>>()
            .join(",");
        message.push_str(&format!(
            "{}|{}|{}|{}: [{}]\n",
            topic.id, topic.name, topic.code, topic.user_id, permissions
        ));
    }

    if let Some(last_topic_id) = last_topic_id {
        let token = telegram_service.add_id_token(last_topic_id).await?;
        message.push_str(&format!("----\nQuery more by token: {token}"));
    } else {
        message.push_str("----\nEnd");
    }

    chat_ctx.send_message(message).await?;
    Ok(())
}

async fn list_pending_user(
    context: &Context,
    telegram_service: &TelegramService<'_>,
    chat_ctx: &ChatContext<'_>,
    Params(token): Params<Option<&str>>,
) -> NotifyExchangeResult<()> {
    let last_pending_user_id = if let Some(token) = token {
        Some(telegram_service.get_id_by_token(token).await?)
    } else {
        None
    };

    let (pending_users, last_pending_user_id) = context
        .user_service()
        .list_pending_user(
            context.db(),
            Cond::all(),
            last_pending_user_id,
            TelegramService::LIMIT,
        )
        .await?;

    let mut message = "Pending Users:\n".to_string();
    for pending_user in pending_users {
        message.push_str(&format!(
            "{}|{}|{}|{}: [{}]\n",
            pending_user.email,
            pending_user.username,
            pending_user.expired_at,
            pending_user.ext,
            pending_user.id,
        ));
    }

    if let Some(last_pending_user_id) = last_pending_user_id {
        let token = telegram_service.add_id_token(last_pending_user_id).await?;
        message.push_str(&format!("----\nQuery more by token: {token}"));
    } else {
        message.push_str("----\nEnd");
    }

    chat_ctx.send_message(message).await?;
    Ok(())
}

async fn list_user(
    context: &Context,
    telegram_service: &TelegramService<'_>,
    chat_ctx: &ChatContext<'_>,
    Params(token): Params<Option<&str>>,
) -> NotifyExchangeResult<()> {
    let last_user_id = if let Some(token) = token {
        Some(telegram_service.get_id_by_token(token).await?)
    } else {
        None
    };

    let (users, last_user_id) = context
        .user_service()
        .list_user(context.db(), last_user_id, TelegramService::LIMIT)
        .await?;

    let mut message = "Users:\n".to_string();
    for user in users {
        message.push_str(&format!(
            "{}|{}|{}: [{}]\n",
            user.email, user.username, user.created_at, user.id,
        ));
    }

    if let Some(last_user_id) = last_user_id {
        let token = telegram_service.add_id_token(last_user_id).await?;
        message.push_str(&format!("----\nQuery more by token: {token}"));
    } else {
        message.push_str("----\nEnd");
    }

    chat_ctx.send_message(message).await?;
    Ok(())
}

async fn list_service(
    context: &Context,
    telegram_service: &TelegramService<'_>,
    chat_ctx: &ChatContext<'_>,
    Params(token): Params<Option<&str>>,
) -> NotifyExchangeResult<()> {
    let last_service_id = if let Some(token) = token {
        Some(telegram_service.get_id_by_token(token).await?)
    } else {
        None
    };

    let (services, last_service_id) = context
        .transport_service_service()
        .list_service(context.db(), last_service_id, TelegramService::LIMIT)
        .await?;

    let mut message = "Services:\n".to_string();
    for service in services {
        message.push_str(&format!(
            "{}|{:?}|{}|{}: [{}]\n",
            service.name,
            service.transport_service_type,
            service.created_at,
            service.description.unwrap_or_default(),
            service.id,
        ));
    }

    if let Some(last_service_id) = last_service_id {
        let token = telegram_service.add_id_token(last_service_id).await?;
        message.push_str(&format!("----\nQuery more by token: {token}"));
    } else {
        message.push_str("----\nEnd");
    }

    chat_ctx.send_message(message).await?;
    Ok(())
}

async fn list_service_endpoint(
    context: &Context,
    telegram_service: &TelegramService<'_>,
    chat_ctx: &ChatContext<'_>,
    Params((service_id, token)): Params<(&str, Option<&str>)>,
) -> NotifyExchangeResult<()> {
    let service_id: Id = service_id
        .parse()
        .map_err(|_| error::invalid_request("Invalid Id"))?;

    let last_endpoint_id = if let Some(token) = token {
        Some(telegram_service.get_id_by_token(token).await?)
    } else {
        None
    };

    let (endpoints, last_endpoint_id) = context
        .endpoint_service()
        .list_endpoint(
            context.db(),
            Cond::all().add(EndpointColumn::TransportServiceId.eq(service_id)),
            last_endpoint_id,
            TelegramService::LIMIT,
        )
        .await?;

    let mut message = "Endpoints:\n".to_string();

    for endpoint in endpoints {
        message.push_str(&format!(
            "{}|{}|{}|{:?}|{}\n",
            endpoint.id,
            endpoint.code,
            endpoint.name,
            endpoint.transport_service_type,
            endpoint.transport_service_id
        ));
    }

    if let Some(last_endpoint_id) = last_endpoint_id {
        let token = telegram_service.add_id_token(last_endpoint_id).await?;
        message.push_str(&format!("----\nQuery more by token: {token}"));
    } else {
        message.push_str("----\nEnd");
    }

    chat_ctx.send_message(message).await?;
    Ok(())
}

async fn list_endpoint_subscription(
    context: &Context,
    telegram_service: &TelegramService<'_>,
    chat_ctx: &ChatContext<'_>,
    Params((endpoint_id, token)): Params<(&str, Option<&str>)>,
) -> NotifyExchangeResult<()> {
    let endpoint_id: Id = endpoint_id
        .parse()
        .map_err(|_| error::invalid_request("Invalid Id"))?;

    list_subscription_base(
        context,
        telegram_service,
        chat_ctx,
        Cond::all().add(SubscriptionColumn::EndpointId.eq(endpoint_id)),
        token,
    )
    .await
}

async fn list_topic_subscription(
    context: &Context,
    telegram_service: &TelegramService<'_>,
    chat_ctx: &ChatContext<'_>,
    Params((topic_id, token)): Params<(&str, Option<&str>)>,
) -> NotifyExchangeResult<()> {
    let topic_id: Id = topic_id
        .parse()
        .map_err(|_| error::invalid_request("Invalid Id"))?;

    list_subscription_base(
        context,
        telegram_service,
        chat_ctx,
        Cond::all().add(SubscriptionColumn::TopicId.eq(topic_id)),
        token,
    )
    .await
}

async fn get_user(
    context: &Context,
    chat_ctx: &ChatContext<'_>,
    Params(user_id): Params<&str>,
) -> NotifyExchangeResult<()> {
    let user_id: Id = user_id
        .parse()
        .map_err(|_| error::invalid_request("Invalid Id"))?;

    let user = context
        .user_service()
        .find_user_by_id(context.db(), user_id)
        .await?;
    let mut message = "Users:\n".to_string();
    message.push_str(&format!(
        "{}|{}|{}: [{}]\n",
        user.email, user.username, user.created_at, user.id,
    ));
    message.push_str("----\nEnd");
    chat_ctx.send_message(message).await?;
    Ok(())
}

async fn get_topic(
    context: &Context,
    chat_ctx: &ChatContext<'_>,
    user: &User,
    Params(topic_id): Params<&str>,
) -> NotifyExchangeResult<()> {
    let topic_id: Id = topic_id
        .parse()
        .map_err(|_| error::invalid_request("Invalid Id"))?;

    let topic = context
        .topic_service()
        .find_by_id(context.db(), topic_id)
        .await?;
    let permissions = context
        .topic_service()
        .find_user_topic_permissions(context.db(), topic_id, user.id)
        .await?
        .iter()
        .map(|tp| tp.permission_type.to_short_str())
        .collect::<Vec<_>>()
        .join(",");
    let mut message = "Topics:\n".to_string();
    message.push_str(&format!(
        "{}|{}|{}|{}: [{}]\n",
        topic.id, topic.name, topic.code, topic.user_id, permissions
    ));
    message.push_str("----\nEnd");
    chat_ctx.send_message(message).await?;
    Ok(())
}

async fn get_subscription(
    context: &Context,
    chat_ctx: &ChatContext<'_>,
    Params(subscription_id): Params<&str>,
) -> NotifyExchangeResult<()> {
    let subscription_id: Id = subscription_id
        .parse()
        .map_err(|_| error::invalid_request("Invalid Id"))?;

    let subscription = context
        .subscription_service()
        .find_by_id(context.db(), subscription_id)
        .await?
        .ok_or_else(|| error::not_found("Subscription not exist"))?;
    let topic = context
        .topic_service()
        .find_by_id(context.db(), subscription.topic_id)
        .await?;
    let endpoint = context
        .endpoint_service()
        .find_by_id(context.db(), subscription.endpoint_id)
        .await?;
    let offset = context
        .subscription_service()
        .find_offset(context.db(), subscription.id)
        .await?;
    let latest_message_id = context
        .topic_service()
        .find_latest_message_id_by_topic(subscription.topic_id)
        .await?;
    let mut message = "Subscriptions:\n".to_string();
    message.push_str(&format!(
        "{}|{}|{}|{}|[{}]|offset[{}/{}]\n",
        subscription.id,
        topic.id,
        topic.code,
        topic.name,
        endpoint
            .as_ref()
            .map(|e| e.name.as_str())
            .unwrap_or("No endpoint"),
        offset.next_message_id - 1,
        latest_message_id.unwrap_or(0)
    ));
    message.push_str("----\nEnd");
    chat_ctx.send_message(message).await?;
    Ok(())
}

async fn delete_expired_pending_user(
    context: &Context,
    chat_ctx: &ChatContext<'_>,
    Params(num): Params<Option<&str>>,
) -> NotifyExchangeResult<()> {
    let num = if let Some(num) = num {
        num.parse::<usize>()
            .map_err(|_| error::invalid_request("Invalid number"))?
    } else {
        10
    };
    if num > 30 {
        return Err(error::invalid_request(
            "The parameter should be less than 30",
        ));
    }

    let (pending_users, _) = context
        .user_service()
        .list_pending_user(
            context.db(),
            Cond::all().add(PendingUserColumn::ExpiredAt.lt(Utc::now())),
            None,
            TelegramService::LIMIT,
        )
        .await?;

    let mut ids = vec![];
    let mut message = "Deleted pending Users:\n".to_string();
    for pending_user in &pending_users {
        ids.push(pending_user.id);
        message.push_str(&format!(
            "{}|{}|{}|{}: [{}]\n",
            pending_user.email,
            pending_user.username,
            pending_user.expired_at,
            pending_user.ext,
            pending_user.id,
        ));
    }
    message.push_str("----\nEnd");

    let num = context
        .transaction(async |conn| context.user_service().delete_pending_user(conn, &ids).await)
        .await?;
    message.push_str(&format!("\nAmount: {}, Deleted: {num}", ids.len()));

    chat_ctx.send_message(message).await?;
    Ok(())
}

async fn delete_all_expired_pending_user(
    context: &Context,
    chat_ctx: &ChatContext<'_>,
) -> NotifyExchangeResult<()> {
    let num = context
        .user_service()
        .delete_all_pending_user(context.db())
        .await?;
    let message = format!("Deleted pending users: {num}");

    chat_ctx.send_message(message).await?;
    Ok(())
}

async fn set_user_enabled(
    context: &Context,
    chat_ctx: &ChatContext<'_>,
    operator: &User,
    Params((user_id, enabled)): Params<(Id, bool)>,
) -> NotifyExchangeResult<()> {
    context
        .user_service()
        .set_user_enabled(context.db(), Cow::Borrowed(operator), user_id, enabled)
        .await?;
    let message = format!("Set user enabled: {enabled}");

    chat_ctx.send_message(message).await?;
    Ok(())
}

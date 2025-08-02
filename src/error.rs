use std::{backtrace::Backtrace, borrow::Cow};

use axum::{
    Json,
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use reqwest::header::WWW_AUTHENTICATE;
use sea_orm::{DbErr, TransactionError};
use serde::Serialize;
use snafu::{FromString, ResultExt, Snafu};
use teloxide::RequestError;
use validator::ValidationErrors;

#[derive(Debug)]
pub enum WwwAuthenticate {
    NeLogin,
    NeWebhook,
    Basic(&'static str),
}

#[derive(Debug, Snafu)]
pub enum NotifyExchangeError {
    #[snafu(display("InvalidVariant: {message}"))]
    InvalidVariant { message: String },
    #[snafu(display("InvalidKey: {message}"))]
    InvalidKey { message: String },
    #[snafu(display("InvalidConfig: {message}"))]
    InvalidConfig { message: String },
    #[snafu(display("NotFound: {message}"))]
    NotFound { message: String },
    #[snafu(display("InternalServerError: {message}"))]
    InternalServerError { message: String },
    #[snafu(display("InvalidRequest: {message}"))]
    InvalidRequest { message: String },
    #[snafu(display("InvalidResponse: {message}"))]
    InvalidResponse { message: String },
    #[snafu(display("InvalidSign: {message}"))]
    InvalidSign { message: String },
    #[snafu(display("Conflict: {message}"))]
    Conflict { message: String },
    #[snafu(display("Forbidden: {message}"))]
    Forbidden { message: String },
    #[snafu(display("Unauthorized: {message}"))]
    Unauthorized {
        message: String,
        www_authenticates: Vec<WwwAuthenticate>,
    },
    #[snafu(display("OverRetryLimit: {message}"))]
    OverRetryLimit { message: String },
    #[snafu(context(false), display("Db: {source:?}"))]
    Db { source: DbErr, backtrace: Backtrace },
    #[snafu(context(false), display("TelegramRequest: {source:?}"))]
    TelegramRequest {
        source: RequestError,
        backtrace: Backtrace,
    },
    #[snafu(context(false), display("Figment: {source:?}"))]
    Figment {
        #[snafu(source(from(figment::Error, Box::new)))]
        source: Box<figment::Error>,
        backtrace: Backtrace,
    },
    #[snafu(context(false), display("Reqwest: {source:?}"))]
    Reqwest {
        source: reqwest::Error,
        backtrace: Backtrace,
    },
    #[snafu(display("SerdeJson: {message}, error: {source:?}"))]
    SerdeJson {
        message: String,
        source: serde_json::Error,
    },
    #[snafu(whatever, display("Whatever: {message}, error: {source:?}"))]
    Whatever {
        message: String,
        #[snafu(source(from(Box<dyn std::error::Error + Send + Sync>, Some)))]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
        #[snafu(backtrace)]
        backtrace: Backtrace,
    },
}

impl<E> From<TransactionError<E>> for NotifyExchangeError
where
    E: Into<NotifyExchangeError>,
{
    fn from(value: TransactionError<E>) -> Self {
        match value {
            TransactionError::Connection(db_err) => db_err.into(),
            TransactionError::Transaction(e) => e.into(),
        }
    }
}

impl From<ValidationErrors> for NotifyExchangeError {
    fn from(errors: ValidationErrors) -> Self {
        fn visit_errors(
            errors: &ValidationErrors,
            field: &mut Vec<Cow<'static, str>>,
            res: &mut Vec<(String, Vec<Cow<'static, str>>)>,
        ) {
            for (field_name, error_kind) in errors.errors() {
                field.push(field_name.clone());
                match error_kind {
                    validator::ValidationErrorsKind::Struct(validation_errors) => {
                        field.push(Cow::Borrowed("."));
                        visit_errors(validation_errors, field, res);
                        field.pop();
                    }
                    validator::ValidationErrorsKind::List(btree_map) => {
                        btree_map.iter().for_each(|(i, e)| {
                            field.push(Cow::Owned(format!("[{i}]")));
                            visit_errors(e, field, res);
                            field.pop();
                        });
                    }
                    validator::ValidationErrorsKind::Field(validation_errors) => {
                        let mut l = Vec::with_capacity(validation_errors.len());
                        for e in validation_errors {
                            if let Some(m) = &e.message {
                                l.push(m.clone());
                            } else {
                                l.push(e.code.clone());
                            }
                        }
                        res.push((field.concat(), l));
                    }
                }
                field.pop();
            }
        }

        let mut res = vec![];

        visit_errors(&errors, &mut vec![], &mut res);

        let message = res
            .iter()
            .map(|(f, errors)| {
                let m = match serde_json::to_string(errors) {
                    Ok(m) => Cow::Owned(m),
                    Err(e) => {
                        tracing::error!("Serialize validation errors failed: {e:?}");
                        Cow::Borrowed("Unknown error")
                    }
                };
                format!("{f}: {m}")
            })
            .collect::<Vec<_>>()
            .join(",");
        invalid_request(format!("ValidationErrors, {message}"))
    }
}

impl NotifyExchangeError {
    pub fn is_invalid_request(&self) -> bool {
        matches!(self, NotifyExchangeError::InvalidRequest { .. })
    }

    /// Because Response isn't cloneable, this method is make it cloneable.
    pub fn into_response_parts(self) -> (StatusCode, HeaderMap, ErrorResponse) {
        let mut header_map = HeaderMap::new();
        let (status, message) = match &self {
            NotifyExchangeError::NotFound { message } => (StatusCode::NOT_FOUND, message.clone()),
            NotifyExchangeError::Conflict { message } => (StatusCode::CONFLICT, message.clone()),
            NotifyExchangeError::Unauthorized {
                message,
                www_authenticates,
            } => {
                for www_authenticate in www_authenticates {
                    let v = match www_authenticate {
                        WwwAuthenticate::NeLogin => HeaderValue::from_static("NeLogin"),
                        WwwAuthenticate::NeWebhook => HeaderValue::from_static("NeWebhook"),
                        WwwAuthenticate::Basic(realm) => {
                            let v = Some(realm)
                                .filter(|r| !r.contains("\"") && !r.is_empty())
                                .and_then(|r| {
                                    HeaderValue::try_from(format!(
                                        r#"Basic realm="{r}", charset="UTF-8""#,
                                    ))
                                    .map_or_else(
                                        |e| {
                                            tracing::error!("Invalid realm: {realm}, error: {e:?}");
                                            Some(HeaderValue::from_static(
                                                r#"Basic charset="UTF-8""#,
                                            ))
                                        },
                                        Some,
                                    )
                                });
                            if let Some(v) = v {
                                v
                            } else {
                                tracing::error!("Invalid realm: {realm}");
                                HeaderValue::from_static(r#"Basic charset="UTF-8""#)
                            }
                        }
                    };
                    header_map.append(WWW_AUTHENTICATE, v);
                }
                (StatusCode::UNAUTHORIZED, message.clone())
            }
            NotifyExchangeError::InvalidSign { .. } => {
                header_map.append(WWW_AUTHENTICATE, HeaderValue::from_static("NeWebhook"));
                (
                    StatusCode::UNAUTHORIZED,
                    StatusCode::UNAUTHORIZED.to_string(),
                )
            }
            NotifyExchangeError::Forbidden { .. } => {
                (StatusCode::FORBIDDEN, StatusCode::FORBIDDEN.to_string())
            }
            NotifyExchangeError::InvalidRequest { message } => {
                (StatusCode::BAD_REQUEST, message.clone())
            }
            NotifyExchangeError::Db { .. } => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Database error occurred".to_string(),
            ),
            _ => (
                StatusCode::INTERNAL_SERVER_ERROR,
                StatusCode::INTERNAL_SERVER_ERROR.to_string(),
            ),
        };
        if status == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!("NotifyExchangeError: {:#?}", self,);
        } else {
            tracing::info!("NotifyExchangeError: {:?}", self,);
        }
        let response = ErrorResponse {
            status: status.as_str().to_string(),
            message,
        };
        (status, header_map, response)
    }
}

#[derive(Clone, Serialize)]
pub struct ErrorResponse {
    status: String,
    message: String,
}

impl IntoResponse for NotifyExchangeError {
    fn into_response(self) -> Response {
        let (status, header_map, response) = self.into_response_parts();
        (status, header_map, Json(response)).into_response()
    }
}

pub type NotifyExchangeResult<T> = Result<T, NotifyExchangeError>;

pub trait NotifyExchangeResultExt<T, E>: Sized {
    fn whatever<S>(self, context: S) -> NotifyExchangeResult<T>
    where
        S: Into<String>;

    #[allow(unused)]
    fn with_whatever<F, S>(self, context: F) -> NotifyExchangeResult<T>
    where
        F: FnOnce(&mut E) -> S,
        S: Into<String>;
}

impl<T, E, R> NotifyExchangeResultExt<T, E> for R
where
    R: ResultExt<T, E>,
    E: Into<<NotifyExchangeError as FromString>::Source>,
{
    fn whatever<S>(self, context: S) -> NotifyExchangeResult<T>
    where
        S: Into<String>,
    {
        self.whatever_context::<S, NotifyExchangeError>(context)
    }

    fn with_whatever<F, S>(self, context: F) -> NotifyExchangeResult<T>
    where
        F: FnOnce(&mut E) -> S,
        S: Into<String>,
    {
        self.with_whatever_context::<F, S, NotifyExchangeError>(context)
    }
}

macro_rules! error_fn {
    ($name:ident, $variant:ident) => {
        pub fn $name<M: Into<String>>(message: M) -> NotifyExchangeError {
            NotifyExchangeError::$variant {
                message: message.into(),
            }
        }
    };
}

error_fn!(conflict, Conflict);
error_fn!(forbidden, Forbidden);
error_fn!(internal_server_error, InternalServerError);
error_fn!(invalid_key, InvalidKey);
error_fn!(invalid_request, InvalidRequest);
error_fn!(invalid_response, InvalidResponse);
error_fn!(invalid_sign, InvalidSign);
error_fn!(invalid_variant, InvalidVariant);
error_fn!(not_found, NotFound);
error_fn!(overy_retry_limit, OverRetryLimit);

pub fn unauthorized<M: Into<String>>(
    message: M,
    www_authenticates: Vec<WwwAuthenticate>,
) -> NotifyExchangeError {
    NotifyExchangeError::Unauthorized {
        message: message.into(),
        www_authenticates,
    }
}

pub fn serde_json_error_cb<M: Into<String>>(
    message: M,
) -> impl FnOnce(serde_json::Error) -> NotifyExchangeError {
    move |e| NotifyExchangeError::SerdeJson {
        message: message.into(),
        source: e,
    }
}

use axum::{
    Extension, Json,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use axum_extra::extract::CookieJar;
use serde::{Deserialize, Serialize, Serializer, ser::SerializeMap as _};
use validator::{Validate, ValidationError};

use crate::{
    context::RequestContext,
    db::custom_type::Id,
    error::{self, NotifyExchangeResult},
};

pub mod admin;
pub mod endpoint;
pub mod topic;
pub mod user;
pub mod webhook;

pub struct Response<T> {
    status: StatusCode,
    headers: Option<HeaderMap>,
    cookies: Option<CookieJar>,
    data: T,
}

impl<T> Response<T> {
    pub fn new(data: T) -> Self {
        Self {
            status: StatusCode::OK,
            headers: None,
            cookies: None,
            data,
        }
    }

    pub fn with_status(mut self, status: StatusCode) -> NotifyExchangeResult<Self> {
        if !status.is_success() {
            return Err(error::internal_server_error(
                "Only success status code are allowed",
            ));
        }
        self.status = status;
        Ok(self)
    }

    pub fn with_headers(mut self, headers: Option<HeaderMap>) -> Self {
        self.headers = headers;
        self
    }

    pub fn with_cookies(mut self, cookies: Option<CookieJar>) -> Self {
        self.cookies = cookies;
        self
    }
}

impl<T> Serialize for Response<T>
where
    T: Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(2))?;

        map.serialize_entry("status", &self.status.to_string())?;
        map.serialize_entry("data", &self.data)?;

        map.end()
    }
}

impl<T> IntoResponse for Response<T>
where
    T: Serialize,
{
    fn into_response(mut self) -> axum::response::Response {
        match (self.headers.take(), self.cookies.take()) {
            (None, None) => (self.status, Json(self)).into_response(),
            (None, Some(cookies)) => (self.status, cookies, Json(self)).into_response(),
            (Some(headers), None) => (self.status, headers, Json(self)).into_response(),
            (Some(headers), Some(cookies)) => {
                (self.status, headers, cookies, Json(self)).into_response()
            }
        }
    }
}

impl<T> From<T> for Response<T> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

#[derive(Deserialize, Validate)]
struct PaginationQuery {
    #[validate(range(min = 1, max = 100))]
    #[serde(default = "default_page")]
    page: usize,
    #[validate(custom(function = "allowed_pagination_size"))]
    #[serde(default = "default_pagination_size")]
    size: usize,
}

impl PaginationQuery {
    pub fn offset(&self) -> usize {
        (self.page - 1) * self.size
    }
}

pub type MaybeSuccessResponse<T> = NotifyExchangeResult<Response<T>>;

pub async fn initialized(
    Extension(req_ctx): Extension<RequestContext>,
) -> NotifyExchangeResult<StatusCode> {
    let count = req_ctx
        .global
        .transport_service_service()
        .count_enabled(req_ctx.global.db())
        .await?;
    if count == 0 {
        Ok(StatusCode::NOT_FOUND)
    } else {
        Ok(StatusCode::OK)
    }
}

pub fn assure_user(this: Id, other: Id) -> NotifyExchangeResult<()> {
    if this != other {
        Err(error::not_found("Not found"))
    } else {
        Ok(())
    }
}

fn allowed_pagination_size(size: usize) -> Result<(), ValidationError> {
    const SIZES: &[usize] = &[5, 10, 25, 50];
    if SIZES.contains(&size) {
        Ok(())
    } else {
        Err(ValidationError::new("Invalid page size")
            .with_message(format!("Page size should be one of {SIZES:?}").into()))
    }
}

fn default_page() -> usize {
    1
}

fn default_pagination_size() -> usize {
    10
}

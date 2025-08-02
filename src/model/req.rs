use std::borrow::Cow;

use serde::{Deserialize, Serialize};
use validator::Validate;

use crate::db::custom_type::{Id, TransportServiceType};

#[derive(Deserialize, Validate)]
pub struct CreateUserRequest<'a> {
    #[validate(length(min = 4, max = 64))]
    pub name: Cow<'a, str>,
    #[validate(length(max = 128))]
    #[validate(email)]
    pub email: Cow<'a, str>,
}

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct CreateEndpointRequest<'a> {
    #[validate(length(min = 4, max = 128))]
    pub name: Cow<'a, str>,
    #[validate(length(min = 4, max = 128), regex(path = *crate::model::CODE_CHARS))]
    pub code: Cow<'a, str>,
    pub transport_service_id: Id,
    pub transport_service_type: TransportServiceType,
    pub options: String,
    #[validate(length(max = 1024))]
    pub description: Option<Cow<'a, str>>,
    pub is_public: bool,
}

#[derive(Deserialize, Validate)]
pub struct CreateTopicRequest<'a> {
    #[validate(length(min = 4, max = 128))]
    pub name: Cow<'a, str>,
    #[validate(length(min = 4, max = 128), regex(path = *crate::model::CODE_CHARS))]
    pub code: Cow<'a, str>,
    #[validate(length(max = 1024))]
    pub description: Option<Cow<'a, str>>,
}

#[derive(Deserialize, Validate)]
pub struct CreateHttpTransportServiceRequest<'a> {
    #[validate(length(min = 4, max = 128))]
    pub name: Cow<'a, str>,
    #[validate(length(max = 1024))]
    pub description: Option<Cow<'a, str>>,
}

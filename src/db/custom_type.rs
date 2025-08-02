use std::{
    fmt::{Display, Formatter, Result as FmtResult},
    str::FromStr,
};

use num_enum::TryFromPrimitive;
use sea_orm::{DbErr, DeriveActiveEnum, DeriveValueType, EnumIter, TryFromU64, Value};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{self, NotifyExchangeError};

#[repr(transparent)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Deserialize, Serialize, DeriveValueType)]
pub struct Id(Uuid);

impl Id {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for Id {
    fn default() -> Self {
        Self::new()
    }
}

impl Display for Id {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        self.0.fmt(f)
    }
}

impl FromStr for Id {
    type Err = NotifyExchangeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<Uuid>()
            .map(Id)
            .map_err(|_| error::invalid_request("Invalid format of id"))
    }
}

/// Impl `TryFromU64` is needed for Id to be a primary key
impl TryFromU64 for Id {
    fn try_from_u64(_: u64) -> Result<Self, DbErr> {
        // Follow the way uuid did
        Err(DbErr::ConvertFromU64("Id"))
    }
}

impl From<&Id> for Value {
    fn from(value: &Id) -> Self {
        (*value).into()
    }
}

pub trait TryFromPrimitiveStr: Sized {
    type Error;

    fn try_from_primitive_str(s: &str) -> Result<Self, Self::Error>;
}

impl<T> TryFromPrimitiveStr for T
where
    T: TryFromPrimitive,
    <T as TryFromPrimitive>::Primitive: FromStr,
{
    type Error = NotifyExchangeError;

    fn try_from_primitive_str(s: &str) -> Result<Self, Self::Error> {
        s.parse::<<T as TryFromPrimitive>::Primitive>()
            .ok()
            .and_then(|p| Self::try_from_primitive(p).ok())
            .ok_or_else(|| {
                error::invalid_variant(format!(
                    "Invalid variant for {}: {s}",
                    <T as TryFromPrimitive>::NAME
                ))
            })
    }
}

#[repr(i8)]
#[derive(
    Debug,
    Copy,
    Clone,
    PartialEq,
    Eq,
    TryFromPrimitive,
    EnumIter,
    DeriveActiveEnum,
    Serialize,
    Deserialize,
)]
#[sea_orm(rs_type = "i8", db_type = "TinyInteger")]
pub enum EncryptionType {
    Plain = 1,
    SaltedAesGcm = 2,
    Rsa = 3,
    AesGcm = 4,
}

#[repr(i8)]
#[derive(
    Debug,
    Copy,
    Clone,
    PartialEq,
    Eq,
    TryFromPrimitive,
    EnumIter,
    DeriveActiveEnum,
    Serialize,
    Deserialize,
)]
#[sea_orm(rs_type = "i8", db_type = "TinyInteger")]
pub enum KeySource {
    Config = 1,
    Db = 2,
}

#[repr(i16)]
#[derive(
    Debug,
    Copy,
    Clone,
    PartialEq,
    Eq,
    TryFromPrimitive,
    EnumIter,
    DeriveActiveEnum,
    Serialize,
    Deserialize,
)]
#[sea_orm(rs_type = "i16", db_type = "SmallInteger")]
pub enum TransportServiceType {
    Telegram = 1,
    Http = 2,
}

#[repr(i8)]
#[derive(
    Debug,
    Copy,
    Clone,
    PartialEq,
    Eq,
    TryFromPrimitive,
    EnumIter,
    DeriveActiveEnum,
    Serialize,
    Deserialize,
    Hash,
)]
#[sea_orm(rs_type = "i8", db_type = "TinyInteger")]
pub enum TopicPermissionType {
    Read = 1,
    Write = 2,
    ReadInPublic = 3,
    ManageRead = 11,
    ManageWrite = 12,
    ManageReadInPublic = 13,
}

impl TopicPermissionType {
    pub fn to_short_str(&self) -> &'static str {
        match self {
            TopicPermissionType::Read => "r",
            TopicPermissionType::Write => "w",
            TopicPermissionType::ReadInPublic => "rp",
            TopicPermissionType::ManageRead => "mr",
            TopicPermissionType::ManageWrite => "mw",
            TopicPermissionType::ManageReadInPublic => "mrp",
        }
    }

    pub fn require(&self) -> Self {
        match self {
            TopicPermissionType::Read => TopicPermissionType::ManageRead,
            TopicPermissionType::Write => TopicPermissionType::ManageWrite,
            TopicPermissionType::ReadInPublic => TopicPermissionType::ManageReadInPublic,
            TopicPermissionType::ManageRead => TopicPermissionType::ManageRead,
            TopicPermissionType::ManageWrite => TopicPermissionType::ManageWrite,
            TopicPermissionType::ManageReadInPublic => TopicPermissionType::ManageReadInPublic,
        }
    }
}

#[repr(i16)]
#[derive(
    Debug,
    Copy,
    Clone,
    PartialEq,
    Eq,
    TryFromPrimitive,
    EnumIter,
    DeriveActiveEnum,
    Serialize,
    Deserialize,
)]
#[sea_orm(rs_type = "i16", db_type = "SmallInteger")]
pub enum LockType {
    Table = 1,
    Biz = 2,
}

#[repr(i8)]
#[derive(
    Debug,
    Copy,
    Clone,
    PartialEq,
    Eq,
    TryFromPrimitive,
    EnumIter,
    DeriveActiveEnum,
    Serialize,
    Deserialize,
)]
#[sea_orm(rs_type = "i8", db_type = "TinyInteger")]
pub enum SubscriptionStatus {
    On = 1,
    OffByError = 2,
}

//! Wire types

use postcard_schema_ng::{Schema, key::Key};
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone, Copy, Schema)]
pub enum Operation {
    Request,
    Response,
    ErrorResponse,
}

// TODO: We might want to manually impl Schema/Serialize/Deserialize for Header,
// otherwise adding new operations will be a breaking schema change. We could also
// implement method as a `u8` and/or u8 wrapper type. Fix this before releasing
#[derive(Serialize, Deserialize, Debug, Clone, Schema)]
pub struct Header {
    // ensure version is first, in case we ever need to introduce breaking
    // changes in the header version and decode this first.
    pub version: u8,
    pub op: Operation,
    pub seqno: u16,
    pub key: Key,
}

#[derive(Serialize, Deserialize, Debug, Clone, Schema)]
pub struct WireError(pub u8);

impl WireError {
    // Server errors
    pub const SERVER_BAD_HEADER: Self = Self(10u8);
    pub const SERVER_UNEXPECTED_METHOD: Self = Self(11u8);
    pub const SERVER_VERSION_MISMATCH: Self = Self(12u8);
    pub const SERVER_UNKNOWN_METHOD: Self = Self(13u8);
    pub const SERVER_BAD_BODY: Self = Self(14u8);
    pub const SERVER_RESPOND_FAILED: Self = Self(15u8);

    pub const KEY: Key = Key::for_path::<Self>("error");
}

impl From<crate::io::server::ServerError> for WireError {
    fn from(value: crate::io::server::ServerError) -> Self {
        match value {
            crate::ServerError::RequestHeaderDeserialize(_) => Self::SERVER_BAD_HEADER,
            crate::ServerError::RequestWrongOperation => Self::SERVER_UNEXPECTED_METHOD,
            crate::ServerError::RequestVersionMismatch => Self::SERVER_VERSION_MISMATCH,
            crate::ServerError::UnknownMethod => Self::SERVER_UNKNOWN_METHOD,
            crate::ServerError::RequestBodyDeserialize(_) => Self::SERVER_BAD_BODY,
            crate::ServerError::ResponseSerialize(_) => Self::SERVER_RESPOND_FAILED,
        }
    }
}

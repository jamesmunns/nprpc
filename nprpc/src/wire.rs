use postcard_schema_ng::{Schema, key::Key};
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone, Copy, Schema)]
pub enum Method {
    Request,
    Response,
    ErrorResponse,
}

// TODO: We might want to manually impl Schema/Serialize/Deserialize for Header,
// otherwise adding new methods will be a breaking schema change. We could also
// implement method as a `u8` and/or u8 wrapper type. Fix this before releasing
#[derive(Serialize, Deserialize, Debug, Clone, Schema)]
pub struct Header {
    // ensure version is first, in case we ever need to introduce breaking
    // changes in the header version and decode this first.
    pub version: u8,
    pub method: Method,
    pub seqno: u16,
    pub key: Key,
}

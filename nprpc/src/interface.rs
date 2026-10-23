//! Interface and Interface Methods

use postcard_schema_ng::{
    Schema,
    key::{
        Key,
        hash::{HashItem, HashOptions, fnv1a64::hasher},
    },
    schema::DataModelType,
};
use serde::{Deserialize, Serialize};

const fn method_hasher<T: Schema, U: Schema>(path: &str) -> Key {
    let mut options = HashOptions::DEFAULT;
    options.hash_max_size = true;
    let items = [
        HashItem::String(path),
        HashItem::Schema(T::SCHEMA),
        HashItem::Schema(U::SCHEMA),
    ];
    Key::from_bytes(hasher(&options, &items))
}

/// A trait containing the metadata of a given method of an method
pub trait Method {
    type Request<'rqst>: Schema + Serialize + Deserialize<'rqst>;
    type Response<'resp>: Schema + Serialize + Deserialize<'resp>;

    const NAME: &'static str;

    const KEY: Key = method_hasher::<Self::Request<'static>, Self::Response<'static>>(Self::NAME);

    const INFO: MethodInfo = MethodInfo {
        name: Self::NAME,
        key: Self::KEY,
        req_schema: <Self::Request<'static> as Schema>::SCHEMA,
        resp_schema: <Self::Response<'static> as Schema>::SCHEMA,
    };
}

/// A struct containing metadata for a given method of an interface
#[derive(Debug, Clone, Copy)]
pub struct MethodInfo {
    pub name: &'static str,
    pub key: Key,
    pub req_schema: &'static DataModelType,
    pub resp_schema: &'static DataModelType,
}

/// A struct containing metadata for a given interface
#[derive(Debug, Clone, Copy)]
pub struct InterfaceInfo {
    pub max_request_size: Option<usize>,
    pub max_response_size: Option<usize>,
    pub methods: &'static [MethodInfo],
}

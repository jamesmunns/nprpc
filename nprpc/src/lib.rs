// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg_attr(not(feature = "std"), no_std)]

pub mod macros;

pub mod interface;
pub mod io;
pub mod wire;

pub use io::client::{ClientError, ClientIoError};
pub use io::server::{ServerError, ServerIoError};

// TODO: Should the `Key`s we generate also hash the Header and Error type to
// ensure complete compatibility? Do we consider this part of the versioning?

/// Not covered by semver, re-exported items for external macros
#[doc(hidden)]
pub mod __private {
    pub use postcard_schema_ng::Schema;
    pub use postcard_schema_ng::key::Key;
    pub use postcard_schema_ng::schema::DataModelType;
    pub use serde::{Deserialize, Serialize};
}

pub struct Request<T> {
    pub hdr: wire::Header,
    pub req: T,
}

pub struct RequestRaw<'data> {
    pub hdr: wire::Header,
    pub rqst: &'data [u8],
}

pub struct Response<U> {
    pub hdr: wire::Header,
    pub resp: U,
}

pub struct ResponseRaw<'data> {
    pub hdr: wire::Header,
    pub resp: &'data [u8],
}

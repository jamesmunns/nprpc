// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

pub mod macros;

pub mod client;
pub mod interface;
pub mod wire;

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

pub struct Response<U> {
    pub hdr: wire::Header,
    pub resp: U,
}

#[derive(Debug, PartialEq)]
pub enum Error {
    WrongMethod,
    Unknown,
    PostcardDeser(postcard::Error),
    PostcardSer(postcard::Error),
    BadSeqno,
    KeyMismatch,
    BadMethod,
    VersionMismatch,
}

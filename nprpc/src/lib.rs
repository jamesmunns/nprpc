// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// # NAMING GUIDE
//
// For the sake of consistency, we use the following naming scheme for types
// and lifetimes. Prefer the 4-letter abbreviations to keep pieces aligned in
// multi-line code.
//
// * "Request"
//      * A message sent by a client, and received by a server
//      * `request`, `rqst`, or `'rqst`
// * "Response"
//      * A message sent by a server, and received by a client
//      * `response`, `resp`, or `'resp`
// * "Header"
//      * Included in every request and response, same type in all items
//      * `header` or `hedr`
// * "Body"
//      * The type-specific payload. Used for both the serialized form as well
//        as the rust-type form.
//      * Always `body`. May have the lifetime `'rqst` or `'resp`.

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
    pub hedr: wire::Header,
    pub body: T,
}

pub struct RequestRaw<'rqst> {
    pub hedr: wire::Header,
    pub body: &'rqst [u8],
}

pub struct Response<U> {
    pub hedr: wire::Header,
    pub body: U,
}

pub struct ResponseRaw<'resp> {
    pub hedr: wire::Header,
    pub body: &'resp [u8],
}

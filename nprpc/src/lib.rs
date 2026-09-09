// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! # Not Postcard-RPC
//!
//! It needs a better name. `nprpc` is a tool for communication that is:
//!
//! * Strongly typed (and Rust focused)
//!     * Uses `postcard-schema` for deriving payload schemas.
//!     * Messages are tagged with an eight-byte hash of the method name,
//!       request schema, and response schema. Hash collisions are caught at
//!       compile time, and Cool Schemas Never Change.
//!     * Uses `serde` and `postcard` are used for serialization and
//!       deserialization.
//!     * `postcard-dyn` can be used for dynamic ser/de, and transcoding frames
//!       to/from JSON (Coming Soon™️).
//! * Strongly ordered
//!     * Client initiates request to Server, then Server replies to Client.
//!     * No concurrent in-flight requests.
//!     * No multi-part requests or responses.
//!     * Currently only blocking client/servers supported.
//!     * At least an async (Tokio) client Coming Soon™️.
//! * Flexible transport and storage
//!     * Can be used with or without a heap, both for frame buffering as well
//!       as request/response data payloads.
//!     * Can work over any transport, clients and servers both have `Io` traits
//!       for custom plumbing.
//!     * For bounded types, maximum message sizes can be calculated, and
//!       necessary buffer sizes are generated as `const`s.
//! * Support for flexible data payloads:
//!     * Variable length messages supported.
//!     * `&[u8]` or `&str`s can be borrowed from the incoming frame (for
//!       requests), or borrowed from the server for the outgoing frame (for
//!       responses).
//!     * Support for "type punning" on either side of the connection, e.g. the
//!       server can send a `&str` and the client can receive an owned `String`.
//! * Doesn't concern itself with reliable delivery (and doesn't attempt to add
//!   reliability, That's On You).
//!
//! ## Lightning Tour
//!
//! We call "a set of remotely callable methods" an "interface". You define
//! an interface using the [`interface!`] macro. You usually do this in a shared
//! `example-api` crate.
//!
//! ```rust
//! use nprpc::interface;
//!
//! interface! {
//!     /// This module named "example" contains everything you need for both
//!     /// the client and server.
//!     mod example {
//!         /// We support doc comments and `#[cfg]`s on methods. Methods are
//!         /// written using a mostly-rust-like syntax.
//!         fn echo(u32) -> u32;
//!     }
//! }
//! ```
//!
//! In order to implement the server for this interface, you need to implement
//! the `example::Server` trait's methods. You can use rust-analyzer's
//! "Implement missing methods" action to do this quickly.
//!
//! ```rust
//! # use nprpc::interface;
//! #
//! # interface! {
//! #     /// This module named "example" contains everything you need for both
//! #     /// the client and server.
//! #     mod example {
//! #         /// We support doc comments and `#[cfg]`s on methods. Methods are
//! #         /// written using a mostly-rust-like syntax.
//! #         fn echo(u32) -> u32;
//! #     }
//! # }
//! use nprpc::Request;
//! // From the `example-api` crate above
//! use example::Server;
//!
//! // Not shown: impl Backend for WireBackend { .. }
//! struct WireBackend {
//!     // ...
//! #   a: WireStorage,
//! #   b: WireIo,
//! }
//!
//! // Server-specific type that contains necessary trait
//! struct ServerImpl {
//!     // ...
//! }
//! # impl ServerImpl { pub fn new() -> Self { Self {} }}
//!
//! // Implementation of `example` defined by `interface!` above
//! impl example::Server for ServerImpl {
//!     fn echo(&mut self, req: Request<u32>) -> u32 {
//!         // `req` contains both the header of the request as well as the
//!         // body, what we defined in the interface! macro. We just copy the
//!         // data back out.
//!         *req.body
//!     }
//! }
//!
//! # struct WireIo;
//! # struct WireStorage;
//! # use nprpc::io::server::{ServerIoError, RawIoFrame};
//! # impl nprpc::io::server::Io for WireIo {
//! #     type Error = ();
//! #     type Meta = ();
//! #     fn recv_one_frame_raw<'data>(&mut self, _: &'data mut [u8])
//! #         -> Result<Option<RawIoFrame<'data, ()>>, ServerIoError<()>>
//! #     {
//! #         Err(nprpc::io::server::ServerIoError::Io(()))
//! #     }
//! #     fn send_one_frame_raw(&mut self, _: RawIoFrame<'_, ()>)
//! #         -> Result<(), ServerIoError<()>> { todo!() }
//! # }
//! # impl nprpc::io::Storage for WireStorage {
//! #     fn buffers(&mut self) -> nprpc::io::StorageView<'_> {
//! #         nprpc::io::StorageView { rqst_buf: &mut [], resp_buf: &mut [] }
//! #     }
//! # }
//! # impl nprpc::io::server::Backend for WireBackend {
//! #     type Io = WireIo;
//! #     type Storage = WireStorage;
//! #     fn parts(&mut self) -> (&mut WireStorage, &mut WireIo) {
//! #         let Self { a, b } = self;
//! #         (a, b)
//! #     }
//! # }
//! #
//! # impl WireBackend {
//! #     fn new() -> Self { Self { a: WireStorage, b: WireIo }}
//! # }
//! #
//! fn main() {
//!     let mut wire = WireBackend::new();
//!     let mut server = ServerImpl::new();
//!
//!     // Server one request, typically done in a loop
//!     let res = server.serve_one(&mut wire);
//!     if let Err(e) = res {
//!         println!("Err: {e:?}");
//!     }
//! }
//!
//! ```
//!
//! As a client, you'll get an extension trait called `Client` that lets you
//! call the methods you defined.
//!
//! ```rust,no_run
//! # use nprpc::interface;
//! #
//! # interface! {
//! #     /// This module named "example" contains everything you need for both
//! #     /// the client and server.
//! #     mod example {
//! #         /// We support doc comments and `#[cfg]`s on methods. Methods are
//! #         /// written using a mostly-rust-like syntax.
//! #         fn echo(u32) -> u32;
//! #     }
//! # }
//! #
//! # struct WireIo;
//! # struct WireStorage;
//! # impl nprpc::io::client::Io for WireIo {
//! #     type Error = ();
//! #     fn send_then_receive_raw_frames<'a>(&mut self, _: &[u8], _: &'a mut [u8])
//! #         -> Result<&'a [u8], ()> { todo!() }
//! # }
//! # impl nprpc::io::Storage for WireStorage {
//! #     fn buffers(&mut self) -> nprpc::io::StorageView<'_> { todo!() }
//! # }
//! # impl nprpc::io::client::Backend for WireBackend {
//! #     type Io = WireIo;
//! #     type Storage = WireStorage;
//! #     fn next_sequence_number(&mut self) -> u16 { todo!() }
//! #     fn parts(&mut self) -> (&mut WireStorage, &mut WireIo) { todo!() }
//! # }
//! // Not shown: impl Backend for WireBackend { .. }
//! struct WireBackend;
//!
//! use nprpc::io::client::ClientIoError;
//! use nprpc::Response;
//!
//! // Pull in the extension trait to get access to the methods for any type
//! // that impls `Backend`
//! use example::Client;
//!
//! fn main() {
//!     let mut backend = WireBackend;
//!     let res: Result<Response<u32>, ClientIoError<_>> = backend.echo(&123);
//!     # drop(res);
//! }
//!
//! ```
//!
//! ## Naming Guide
//!
//! For the sake of consistency, we use the following naming scheme for types
//! and lifetimes. Prefer the 4-letter abbreviations to keep pieces aligned in
//! multi-line code.
//!
//! * "Request"
//!      * A message sent by a client, and received by a server
//!      * `request`, `rqst`, or `'rqst`
//! * "Response"
//!      * A message sent by a server, and received by a client
//!      * `response`, `resp`, or `'resp`
//! * "Header"
//!      * Included in every request and response, same type in all items
//!      * `header` or `hedr`
//! * "Body"
//!      * The type-specific payload. Used for both the serialized form as well
//!        as the rust-type form.
//!      * Always `body`. May have the lifetime `'rqst` or `'resp`.

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

/// Borrowed view of a request
///
/// Contains a reference to a request header and request body.
#[derive(Debug, Clone, Copy)]
pub struct Request<'rqst, T> {
    pub hedr: &'rqst wire::Header,
    pub body: &'rqst T,
}

/// Borrowed view of a request
///
/// Like `Request`, but the body has not been deserialized.
#[derive(Debug, Clone, Copy)]
pub struct RequestRaw<'rqst> {
    pub hedr: &'rqst wire::Header,
    pub body: &'rqst [u8],
}

/// Owned view of a response
#[derive(Debug)]
pub struct Response<U> {
    pub hedr: wire::Header,
    pub body: U,
}

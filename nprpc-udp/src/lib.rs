// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! # nprpc over UDP
//!
//! Blocking [`nprpc`] clients and servers using [`std::net::UdpSocket`]. Each
//! request and response is sent as a single datagram, and like `nprpc` itself,
//! nothing is retried: a lost datagram shows up as a timeout on the client.
//!
//! ## Client
//!
//! ```rust,no_run
//! use std::time::Duration;
//!
//! use nprpc::{autobuffer, interface};
//! use nprpc_udp::UdpClient;
//!
//! interface! {
//!     mod example {
//!         fn echo(u32) -> u32;
//!     }
//! }
//! autobuffer!(Bufs, example);
//!
//! use example::Client as _;
//!
//! let mut client = UdpClient::connect("[::1]:5501".parse()?, Bufs::new())?;
//! client.set_timeout(Some(Duration::from_secs(1)))?;
//! assert_eq!(client.echo(&42).unwrap().body, 42);
//! # Ok::<_, Box<dyn std::error::Error>>(())
//! ```
//!
//! ## Server
//!
//! ```rust,no_run
//! use nprpc::{Request, autobuffer, interface};
//! use nprpc_udp::UdpServer;
//!
//! interface! {
//!     mod example {
//!         fn echo(u32) -> u32;
//!     }
//! }
//! autobuffer!(Bufs, example);
//!
//! struct Echo;
//!
//! impl example::Server for Echo {
//!     fn echo(&mut self, rqst: Request<u32>) -> u32 {
//!         *rqst.body
//!     }
//! }
//!
//! let mut udp = UdpServer::bind("[::1]:5501", Bufs::new())?;
//! loop {
//!     let _ = <Echo as example::Server>::serve_one(&mut Echo, &mut udp);
//! }
//! # Ok::<_, std::io::Error>(())
//! ```

use std::io;

use nprpc::io::{Storage, StorageView};

pub mod client;
pub mod server;

pub use client::UdpClient;
pub use server::UdpServer;

/// The largest UDP payload, over IPv6 without jumbograms.
///
/// This is also larger than the largest payload over IPv4 (65,507 bytes), so
/// a response buffer of this size can hold any response.
pub const MAX_UDP_PAYLOAD: usize = 65_527;

/// Request and response buffers, sized at runtime
///
/// Useful when sizes aren't known at compile time, e.g. for a client making
/// calls from a schema it loaded at runtime. Otherwise, prefer buffers sized
/// with [`nprpc::autobuffer!`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VecStorage {
    pub rqst_buf: Vec<u8>,
    pub resp_buf: Vec<u8>,
}

impl VecStorage {
    /// Makes zeroed buffers of the given sizes.
    ///
    /// Both sizes include the [`HEADER_LEN`](nprpc::wire::HEADER_LEN) byte
    /// header.
    pub fn new(rqst_len: usize, resp_len: usize) -> Self {
        Self {
            rqst_buf: vec![0; rqst_len],
            resp_buf: vec![0; resp_len],
        }
    }
}

impl Storage for VecStorage {
    fn buffers(&mut self) -> StorageView<'_> {
        let Self { rqst_buf, resp_buf } = self;
        StorageView { rqst_buf, resp_buf }
    }
}

/// Returns whether an error is a socket timing out.
///
/// Depending on the platform, a socket's read timeout expiring is reported as
/// either [`WouldBlock`](io::ErrorKind::WouldBlock) or
/// [`TimedOut`](io::ErrorKind::TimedOut).
pub fn is_timeout(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}

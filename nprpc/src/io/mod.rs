//! Client and Server I/O items

pub mod client;
pub mod server;

/// A view of a pair of buffers
///
/// This is a struct just to give names to the fields and prevent accidentally
/// swapping the two.
pub struct StorageView<'a> {
    pub rqst_buf: &'a mut [u8],
    pub resp_buf: &'a mut [u8],
}

/// Space for requests and responses.
///
/// This trait is typically implemented using buffers that were calculated
/// using the [`autobuffer!`](crate::autobuffer) macro.
///
/// The same [`Storage`] trait is used for both [`client`s](crate::io::client)
/// and [`server`s](crate::io::server).
pub trait Storage {
    // TODO: do we want this to be postcard-core flavors?
    fn buffers(&mut self) -> StorageView<'_>;
}

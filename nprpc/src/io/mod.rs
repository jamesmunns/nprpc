pub mod client;
pub mod server;

pub struct StorageView<'a> {
    pub rqst_buf: &'a mut [u8],
    pub resp_buf: &'a mut [u8],
}

pub trait Storage {
    // TODO: do we want this to be postcard-core flavors?
    fn buffers(&mut self) -> StorageView<'_>;
}

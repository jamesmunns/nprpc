use crate::Endpoint;
use postcard_schema::key::Key;

pub const fn endpoint_key<E: Endpoint>() -> Key {
    let req_half = Key::for_path::<E::Request>(E::NAME);
    let rsp_half = Key::for_path::<E::Response>(E::NAME);
    let req_bytes = req_half.to_bytes();
    let rsp_bytes = rsp_half.to_bytes();
    let mut out = [0u8; 8];
    let mut idx = 0;
    while idx < 8 {
        out[idx] = req_bytes[idx] ^ rsp_bytes[idx];
        idx += 1;
    }
    unsafe { Key::from_bytes(out) }
}

use postcard_schema::{Schema, key::Key};

use crate::endpoint_key;

pub trait Endpoint {
    const NAME: &'static str;
    type Request: Schema;
    type Response: Schema;
}

macro_rules! endpoints {
    ($(
        $(#[cfg($meta:meta)])?
        $name: ident:
        $req:ty
        =>
        $resp:ty$(,)?
    )*) => {{
        const LIST: &[EndpointInfo] = &[$(
            $(#[cfg($meta)])?
            const {
                struct Boop {}
                impl Endpoint for Boop {
                    const NAME: &'static str = stringify!($name);
                    type Request = $req;
                    type Response = $resp;
                }
                EndpointInfo::info::<Boop>()
            },
        )*];
        LIST
    }};
}

const ENDPOINTS: &[EndpointInfo] = endpoints!(
    #[cfg(not(feature = "std"))] lol: u32 => u32,
    lmao: u32 => u32,
    // "lol": u32 => u32,
);

pub struct EndpointInfo {
    pub name: &'static str,
    pub key: Key,
}

impl EndpointInfo {
    pub const fn info<E: Endpoint>() -> Self {
        Self {
            name: E::NAME,
            key: endpoint_key::<E>(),
        }
    }
}

pub trait Interface {
    const LIST: &'static [EndpointInfo];
}

impl<E: Endpoint> Interface for E {
    const LIST: &'static [EndpointInfo] = &[EndpointInfo::info::<E>()];
}

// impl<E: Endpoint, const N: usize> Interface for [E; N] {
//     const LIST: &'static [EndpointInfo] = const {
//         const ONE: EndpointInfo = EndpointInfo {
//             name: "",
//             key: unsafe { Key::from_bytes([0u8; 8]) },
//         };
//         const ARR: [EndpointInfo; N] = [ONE; N];
//         &ARR
//     };
// }

const fn count_endpoints(interfaces: &'static [&'static [EndpointInfo]]) -> usize {
    let mut idx = 0;
    let mut ct = 0;
    while idx < interfaces.len() {
        ct += interfaces[idx].len();
        idx += 1;
    }
    ct
}

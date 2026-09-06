// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use postcard::{
    Deserializer, Serializer,
    de_flavors::Slice as DeSlice,
    ser_flavors::{Flavor, Slice as SerSlice},
};
use postcard_schema_ng::{Schema, key::Key, schema::DataModelType};
use serde::{Deserialize, Serialize};

#[doc(hidden)]
pub mod __private {
    pub use postcard_schema_ng::Schema;
    pub use postcard_schema_ng::key::Key;
    pub use postcard_schema_ng::schema::DataModelType;
}

/////////////////////////////////////////////////////////
// EXAMPLES
/////////////////////////////////////////////////////////

// TODO: we could dedupe merge_keylists and merge_schemalists by making a macro
// that looks like:
//
// ```rust
// merge_lists! {
//      Key,                        // Type
//      Key::from_bytes([0u8; 8]),  // Initial const value expr
//      [
//          $(...)*                 // Array expansion goes here
//      ]
// }
// ```

/// Internal macro for merging generated keylists, used for
/// `compose_interfaces!`
#[macro_export]
macro_rules! merge_keylists {
    ($($($segment:ident)::+$(,)?)*) => {{
        use $crate::__private::Key;
        const TOTAL_LEN: usize = $crate::count_nested::<Key>(
            &[$(
                $($segment)::+,
            )*]
        );
        const ALL_KEYS_MERGED: [Key; TOTAL_LEN] = const {
            const ONE: Key = {
                Key::from_bytes([0u8; 8])
            };
            let mut arr = [ONE; TOTAL_LEN];
            let mut idx_arr = 0;
            $(
                let mut chidx = 0;
                while chidx < $($segment)::+.len() {
                    arr[idx_arr] = $($segment)::+[chidx];
                    chidx += 1;
                    idx_arr += 1;
                }
            )*
            assert!(idx_arr == TOTAL_LEN);
            arr
        };
        const SLI: &[Key] = &ALL_KEYS_MERGED;
        SLI
    }};
}

/// Internal macro for merging generated schemalists, used for
/// `compose_interfaces!`
#[macro_export]
macro_rules! merge_schemalists {
    ($($($segment:ident)::+$(,)?)*) => {{
        use postcard_schema_ng::schema::DataModelType;
        const TOTAL_LEN: usize = $crate::count_nested::<&DataModelType>(
            &[$(
                $($segment)::+,
            )*]
        );
        const ALL_SCHEMAS_MERGED: [&DataModelType; TOTAL_LEN] = const {
            const ONE: &DataModelType = &DataModelType::Unit;
            let mut arr = [ONE; TOTAL_LEN];
            let mut idx_arr = 0;
            $(
                let mut chidx = 0;
                while chidx < $($segment)::+.len() {
                    arr[idx_arr] = $($segment)::+[chidx];
                    chidx += 1;
                    idx_arr += 1;
                }
            )*
            assert!(idx_arr == TOTAL_LEN);
            arr
        };
        const SLI: &[&DataModelType] = &ALL_SCHEMAS_MERGED;
        SLI
    }};
}

/// ```rust,ignore
/// interface! {
///      mod: ops,
///      | method            | request       | response      |
///      | ------            | -------       | --------      |
///      | mult_three        | u32           | u32           |
/// }
/// ```
#[macro_export]
macro_rules! interface {
    (
        mod: $mod_name:ident,
        | method      | request                                  | response                                      |
        | $(-)*       | $(-)*                                    | $(-)*                                         |
     $( | $mthd:ident | $req_ty:tt $(< $($req_lt:lifetime),+ >)? | $resp_ty:tt $(< $($resp_lt:lifetime),+ >)?    |)*
 ) => {
        /// Module containing server and client
        pub mod $mod_name {
            #[allow(unused_imports)]
            use super::*;

            pub mod schemas {
                #[allow(unused_imports)]
                use super::*;
                use $crate::__private::DataModelType;
                use $crate::__private::Schema;

                // All schemas, un-deduplicated
                pub const ALL_REQ_SCHEMAS: &[&DataModelType] = &[
                    $(
                        $req_ty::SCHEMA,
                    )*
                ];
                pub const ALL_RESP_SCHEMAS: &[&DataModelType] = &[
                    $(
                        $resp_ty::SCHEMA,
                    )*
                ];

                pub const MAX_REQ_SIZE: Option<usize> = $crate::max_buf_required(ALL_REQ_SCHEMAS);
                pub const MAX_RESP_SIZE: Option<usize> = $crate::max_buf_required(ALL_RESP_SCHEMAS);
            }

            /// Calculated keys for all methods in this interface
            pub mod keys {
                #[allow(unused_imports)]
                use super::*;
                use $crate::__private::Key;

                $(
                    #[allow(non_upper_case_globals)]
                    pub const $mthd: Key = $crate::__private::Key::for_2ty_path::<$req_ty, $resp_ty>(
                        stringify!($mthd)
                    );
                )*

                pub const ALL_KEYS: &[Key] = &[
                    $(
                        $mthd,
                    )*
                ];
            }

            pub trait Server {
                // Generate all of the user-filled methods
                $(
                    fn $mthd<$($($req_lt,)+)? $($($resp_lt,)+)?>(
                        &mut self,
                        req: $crate::Request<$req_ty $(< $($req_lt),+ >)? >,
                    ) -> $resp_ty $(< $($resp_lt),+ >)?;
                )*

                /// This method is the prime dispatcher. It takes a processed header and raw body,
                /// and dispatches it to a method if there is a matching one, otherwise returning
                /// Err(Unknown) if the key didn't match.
                fn process_one<'buf>(
                    &mut self,
                    hdr: $crate::Header,
                    body: &[u8],
                    output: &'buf mut [u8],
                ) -> Result<&'buf [u8], $crate::Error> {
                    // This block ensures that all request types implement the Schema trait and
                    // the Deserialize trait, giving a more predictable error if not.
                    $(
                        const _: () = $crate::assert_impls_schema::<$req_ty>();
                        const _: () = $crate::assert_impl_deserialize::<'_, $req_ty>();
                    )*
                    // This block ensures that all response types implement the Schema trait and
                    // the Serialize trait, giving a more predictable error if not.
                    $(
                        const _: () = $crate::assert_impls_schema::<$resp_ty>();
                        const _: () = $crate::assert_impl_serialize::<$resp_ty>();
                    )*

                    // This block ensures that there are no key collisions in all endpoints
                    const _: () = $crate::assert_unique(keys::ALL_KEYS);

                    // Dispatch based on the received key. We trampoline through a monomorphized
                    // version of the `process` function, which handles the common deserialize,
                    // call, serialize portion of the code. This is stamped out on a per-endpoint
                    // basis.
                    match hdr.key {
                        $(
                            keys::$mthd => $crate::process_endpoint::<
                                Self,
                                $req_ty,
                                $resp_ty,
                            >(self, hdr, body, output, <Self as Server>::$mthd),
                        )*

                        // None of the keys matched, return an error.
                        _ => Err($crate::Error::Unknown),
                    }
                }
            }
            pub trait Client {
                $(
                    #[allow(clippy::ptr_arg, clippy::needless_lifetimes)]
                    fn $mthd<'me, $($($req_lt,)+)? $($($resp_lt,)+)?>(
                        &'me mut self,
                        req: &$req_ty $(< $($req_lt),+ >)?
                    ) -> Result<
                        $crate::Response<$resp_ty $(< $($resp_lt),+ >)?,>,
                        $crate::Error,
                    >
                    where
                        $($('me: $resp_lt,)+)?
                    ;
                )*
            }

            impl<T> Client for T
            where
                T: $crate::Backend,
            {
                $(
                    #[allow(clippy::ptr_arg, clippy::needless_lifetimes)]
                    fn $mthd<'me, $($($req_lt,)+)? $($($resp_lt,)+)?>(
                        &'me mut self,
                        req: &$req_ty $(< $($req_lt),+ >)?
                    ) -> Result<
                        $crate::Response<$resp_ty $(< $($resp_lt),+ >)?,>,
                        $crate::Error,
                    >
                    where
                        $($('me: $resp_lt,)+)?
                    {
                        self.send_reply::<$req_ty, $resp_ty>(
                            keys::$mthd,
                            req,
                        )
                    }
                )*
            }
        }
    };
}

/// ```rust,ignore
/// compose_interfaces! {
///      mod: composite,
///      interfaces: [
///          basic,
///          two,
///          ops,
///      ]
/// }
/// ```
#[macro_export]
macro_rules! compose_interfaces {
    (
        mod: $mod_name:ident,
        interfaces: [
            $(
                $($segment:ident)::+$(,)?
            )*
        ]
    ) => {
        pub mod $mod_name {
            pub mod schemas {
                use $crate::__private::DataModelType;
                pub const ALL_REQ_SCHEMAS: &[&DataModelType] = $crate::merge_schemalists!(
                    $(
                        $($segment)::+::schemas::ALL_REQ_SCHEMAS,
                    )*
                );
                pub const ALL_RESP_SCHEMAS: &[&DataModelType] = $crate::merge_schemalists!(
                    $(
                        $($segment)::+::schemas::ALL_RESP_SCHEMAS,
                    )*
                );

                pub const MAX_REQ_SIZE: Option<usize> = $crate::max_buf_required(ALL_REQ_SCHEMAS);
                pub const MAX_RESP_SIZE: Option<usize> = $crate::max_buf_required(ALL_RESP_SCHEMAS);
            }

            pub mod keys {
                use $crate::__private::Key;
                pub const ALL_KEYS: &[Key] = $crate::merge_keylists!(
                    $(
                        $($segment)::+::keys::ALL_KEYS,
                    )*
                );
            }

            pub trait Server: $($($segment)::+::Server+)* {
                fn process_one<'buf>(
                    &mut self,
                    hdr: $crate::Header,
                    body: &[u8],
                    output: &'buf mut [u8],
                ) -> Result<&'buf [u8], $crate::Error>;
            }

            impl<T> Server for T
            where
                $(T: $($segment)::+::Server,)*
            {
                fn process_one<'buf>(
                    &mut self,
                    hdr: $crate::Header,
                    body: &[u8],
                    output: &'buf mut [u8],
                ) -> Result<&'buf [u8], $crate::Error> {
                    // Check all the merged keys to make sure that none of the composed
                    // endpoints have a collision
                    const _: () = $crate::assert_unique(keys::ALL_KEYS);

                    $(
                        // We know that all keys are unique, so instead of pre-checking whether a
                        // sub-interface can handle a request, we just give it to each sub-interface
                        // one at a time until one responds with a non-Unknown result, or we have
                        // exhausted all outcomes.
                        //
                        // TODO: j/k, that causes borrow errors?
                        if $($segment)::+::keys::ALL_KEYS.contains(&hdr.key) {
                            return <Self as $($segment)::+::Server>::process_one(self, hdr, body, output);
                        }
                    )*

                    Err($crate::Error::Unknown)
                }
            }
        }
    };
}

#[macro_export]
macro_rules! autobuffer {
    ($name:ident, $($segment:ident)::+) => {


        pub struct $name {
            pub inc: [u8; Self::_REQ_SIZE],
            pub out: [u8; Self::_RESP_SIZE],
        }

        impl $name {
            const _REQ_SIZE: usize = $($segment)::+::schemas::MAX_REQ_SIZE
                .expect("Unable to automatically size buffer. \
                    One or more request types don't have a max size.");
            const _RESP_SIZE: usize = $($segment)::+::schemas::MAX_RESP_SIZE
                .expect("Unable to automatically size buffer. \
                    One or more response types don't have a max size.");

            pub const fn new() -> Self {
                Self {
                    inc: [0u8; Self::_REQ_SIZE],
                    out: [0u8; Self::_RESP_SIZE],
                }
            }
        }

        impl $crate::Storage for $name {
            fn buffers(&mut self) -> (&mut [u8], &mut [u8]) {
                let Self { inc, out } = self;
                (inc, out)
            }
        }
    };
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone, Copy, Schema)]
pub enum Method {
    Request,
    Response,
}

// TODO: We might want to manually impl Schema/Serialize/Deserialize for Header,
// otherwise adding new methods will be a breaking schema change. We could also
// implement method as a `u8` and/or u8 wrapper type. Fix this before releasing
#[derive(Serialize, Deserialize, Debug, Clone, Schema)]
pub struct Header {
    pub method: Method,
    pub version: u8,
    pub seqno: u16,
    pub key: Key,
}
pub struct Request<T> {
    pub hdr: Header,
    pub req: T,
}
pub struct Response<U> {
    pub hdr: Header,
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

pub fn process_endpoint<'out, 'de, S: ?Sized, Q, R>(
    server: &mut S,
    mut hdr: Header,
    body: &'de [u8],
    out: &'out mut [u8],
    apply: fn(&mut S, Request<Q>) -> R,
) -> Result<&'out [u8], Error>
where
    Q: Deserialize<'de>,
    R: Serialize,
{
    // Deserialize
    let body: Q = postcard::from_bytes(body).map_err(Error::PostcardDeser)?;
    // Process request
    let req = Request {
        hdr: hdr.clone(),
        req: body,
    };
    let resp = apply(server, req);
    // Serialize response
    let mut out = Serializer {
        output: SerSlice::new(out),
    };
    hdr.method = Method::Response;
    hdr.serialize(&mut out).map_err(Error::PostcardSer)?;
    resp.serialize(&mut out).map_err(Error::PostcardSer)?;
    let used = out.output.finalize().map_err(Error::PostcardSer)?;
    Ok(used)
}

pub trait Backend {
    type Storage: Storage;
    type Interface: Interface;
    fn next_sequence_number(&mut self) -> u16;
    fn parts(&mut self) -> (&mut Self::Storage, &mut Self::Interface);
    fn send_reply<'de, Q, R>(&'de mut self, key: Key, req: &Q) -> Result<Response<R>, Error>
    where
        Q: Serialize,
        R: Deserialize<'de> + 'de,
    {
        let seqno = self.next_sequence_number();
        let (sto, intfc) = self.parts();
        let (out, inc) = sto.buffers();

        // SERIALIZE OUTGOING...
        let hdrout = Header {
            method: Method::Request,
            version: 0,
            seqno,
            key,
        };
        let mut out = Serializer {
            output: SerSlice::new(out),
        };
        hdrout.serialize(&mut out).map_err(Error::PostcardSer)?;
        req.serialize(&mut out).map_err(Error::PostcardSer)?;

        // Exchange...
        // TODO: CRC? Wrapping flavor?
        let used = out.output.finalize().map_err(Error::PostcardSer)?;

        // DESERIALIZE INCOMING
        let recvd = intfc.send_reply_raw(used, inc)?;
        let mut inc = Deserializer::from_flavor(DeSlice::new(recvd));
        let hdrin = Header::deserialize(&mut inc).map_err(Error::PostcardDeser)?;

        if hdrin.seqno != hdrout.seqno {
            return Err(Error::BadSeqno);
        }
        if hdrin.method != Method::Response {
            return Err(Error::BadMethod);
        }
        if hdrin.version != hdrout.version {
            return Err(Error::VersionMismatch);
        }
        if hdrin.key != hdrout.key {
            return Err(Error::KeyMismatch);
        }

        // Happy with the header, get the body
        let body = R::deserialize(&mut inc).map_err(Error::PostcardDeser)?;

        Ok(Response {
            hdr: hdrin,
            resp: body,
        })
    }
}

pub trait Storage {
    // TODO: do we want this to be postcard-core flavors?
    fn buffers(&mut self) -> (&mut [u8], &mut [u8]);
}

pub trait Interface {
    fn send_reply_raw<'a>(
        &mut self,
        outgoing: &[u8],
        incoming: &'a mut [u8],
    ) -> Result<&'a [u8], Error>;
}

/////////////////////////////////////////////////////////
// CONST HELPERS
/////////////////////////////////////////////////////////

pub const fn assert_impls_schema<T: postcard_schema_ng::Schema>() {}
pub const fn assert_impl_serialize<T: serde::Serialize>() {}
pub const fn assert_impl_deserialize<'d, T: serde::Deserialize<'d>>() {}
pub const fn assert_unique(keys: &[Key]) {
    let mut i = 0;
    while i < keys.len() {
        let mut j = i + 1;
        while j < keys.len() {
            if i != j {
                let mut matches = true;
                let mut k = 0;
                let a = keys[i].to_bytes();
                let b = keys[j].to_bytes();
                while k < a.len() {
                    if a[k] != b[k] {
                        matches = false;
                        break;
                    }
                    k += 1;
                }
                if matches {
                    panic!("Key collision!");
                }
            }
            j += 1;
        }
        i += 1;
    }
}

pub const fn count_nested<T>(ts: &[&[T]]) -> usize {
    let mut idx = 0;
    let mut ct = 0;
    while idx < ts.len() {
        ct += ts[idx].len();
        idx += 1;
    }
    ct
}

pub const fn max_buf_required(schemas: &[&DataModelType]) -> Option<usize> {
    use postcard_schema_ng::Schema;
    let header_size = Header::SCHEMA.max_size().unwrap();
    let mut max = 0;
    let mut idx = 0;
    while idx < schemas.len() {
        let Some(m) = schemas[idx].max_size() else {
            return None;
        };
        if m > max {
            max = m;
        }
        idx += 1;
    }
    Some(max + header_size)
}

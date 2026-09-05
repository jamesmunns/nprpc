// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use postcard_schema_ng::{Schema, key::Key};
use serde::{Deserialize, Serialize};

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
        use postcard_schema_ng::key::Key;
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
            use super::*;

            pub mod schemas {
                #[allow(unused_imports)]
                use super::*;
                use postcard_schema_ng::schema::DataModelType;

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
            }

            /// Calculated keys for all methods in this interface
            pub mod keys {
                #[allow(unused_imports)]
                use super::*;
                use postcard_schema_ng::key::Key;

                $(
                    #[allow(non_upper_case_globals)]
                    pub const $mthd: Key = $crate::endpoint_key2::<$req_ty, $resp_ty>(
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
                ) -> Result<&'buf mut [u8], $crate::Error> {
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
                        _ => Err(Error::Unknown),
                    }
                }
            }
            pub trait Client {}
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
            $($intfc:ident$(,)?)*
        ]
    ) => {
        pub mod $mod_name {
            pub mod schemas {
                use postcard_schema_ng::schema::DataModelType;
                pub const ALL_REQ_SCHEMAS: &[&DataModelType] = $crate::merge_schemalists!(
                    $(
                        super::super::$intfc::schemas::ALL_REQ_SCHEMAS,
                    )*
                );
                pub const ALL_RESP_SCHEMAS: &[&DataModelType] = $crate::merge_schemalists!(
                    $(
                        super::super::$intfc::schemas::ALL_RESP_SCHEMAS,
                    )*
                );
            }

            pub mod keys {
                use postcard_schema_ng::key::Key;
                pub const ALL_KEYS: &[Key] = $crate::merge_keylists!(
                    $(
                        super::super::$intfc::keys::ALL_KEYS,
                    )*
                );
            }

            pub trait Server: $(super::$intfc::Server+)* {
                fn process_one<'buf>(
                    &mut self,
                    hdr: $crate::Header,
                    body: &[u8],
                    output: &'buf mut [u8],
                ) -> Result<&'buf mut [u8], $crate::Error>;
            }

            impl<T> Server for T
            where
                $(T: super::$intfc::Server,)*
            {
                fn process_one<'buf>(
                    &mut self,
                    hdr: $crate::Header,
                    body: &[u8],
                    output: &'buf mut [u8],
                ) -> Result<&'buf mut [u8], $crate::Error> {
                    // Check all the merged keys to make sure that none of the composed
                    // endpoints have a collision
                    const _: () = $crate::assert_unique(keys::ALL_KEYS);

                    $(
                        if super::$intfc::keys::ALL_KEYS.contains(&hdr.key) {
                            return <Self as super::$intfc::Server>::process_one(self, hdr, body, output);
                        }
                    )*
                    Err($crate::Error::Unknown)
                }
            }
        }
    };
}

pub enum Method {
    Request,
    Response,
}

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

#[derive(Debug, PartialEq)]
pub enum Error {
    WrongMethod,
    Unknown,
    PostcardDeser(postcard::Error),
    PostcardSer(postcard::Error),
}

pub fn process_endpoint<'out, 'de, S: ?Sized, Q, R>(
    server: &mut S,
    hdr: Header,
    body: &'de [u8],
    out: &'out mut [u8],
    apply: fn(&mut S, Request<Q>) -> R,
) -> Result<&'out mut [u8], Error>
where
    Q: Deserialize<'de>,
    R: Serialize,
{
    // Deserialize
    let body: Q = postcard::from_bytes(body).map_err(Error::PostcardDeser)?;
    // Process request
    let req = Request { hdr, req: body };
    let resp = apply(server, req);
    // Serialize response (TODO, we also need to do the header)
    let used = postcard::to_slice(&resp, out).map_err(Error::PostcardSer)?;
    Ok(used)
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

pub const fn half_fold(i: [u8; 8]) -> [u8; 4] {
    let [a, b, c, d, e, f, g, h] = i;
    [a ^ e, b ^ f, c ^ g, d ^ h]
}

// TODO: This kind of sucks, we probably want something like `Key::for_path<A, B>(path)`
// that does full hashing instead of this mixing stuff. DO NOT just straight xor
// `req_half ^ rsp_half`, if both types are the same then they just cancel out!
pub const fn endpoint_key2<Q: Schema, R: Schema>(name: &str) -> Key {
    let req_half = Key::for_path::<Q>(name);
    let rsp_half = Key::for_path::<R>(name);
    let req_bytes = req_half.to_bytes();
    let half_req = half_fold(req_bytes);
    let rsp_bytes = rsp_half.to_bytes();
    let half_rsp = half_fold(rsp_bytes);

    let mut out = [0u8; 8];
    let mut idx = 0;
    while idx < 4 {
        out[idx] = half_req[idx];
        idx += 1;
    }
    while idx < 8 {
        out[idx] = half_rsp[idx - 4];
        idx += 1;
    }
    Key::from_bytes(out)
}

const fn count_nested<T>(ts: &[&[T]]) -> usize {
    let mut idx = 0;
    let mut ct = 0;
    while idx < ts.len() {
        ct += ts[idx].len();
        idx += 1;
    }
    ct
}

/////////////////////////////////////////////////////////
// NOTES AND DEAD CODES
/////////////////////////////////////////////////////////

/* // compose_interfaces! expands to:
 * mod {
 *      trait Client: basic::Client + two::Client {}
 *      impl<B: Backend> Client for B
 *      where
 *          B: basic::Client,
 *          B: two::Client,
 *      {}
 *
 *      trait Server: basic::Server + two::Server {}
 *      impl<T> Server for T
 *      where
 *          T: basic::Server,
 *          T: two::Server,
 *      {
 *          fn process_one<S: Server>(
 *               server: &mut S,
 *               hdr: Header,
 *               body: &[u8],
 *               output: &mut [u8],
 *          ) -> usize {
 *              const MAP: &[(&[EndpointInfo], fn(...) -> usize)] = &[
 *                  (<T as basic::Server>::SCHEMAS, <T as basic::Server>::process_one::<S>),
 *                  (<T as two::Server>::SCHEMAS, <T as two::Server>::process_one::<S>),
 *              ];
 *              for (s, f) in MAP.iter() {
 *                  if s.contains(hdr.key) {
 *                      return f(server, hdr, body, output);
 *                  }
 *              }
 *              // whatever serialize an err here
 *      }
 * }
 *
 * trait Backend {
 *      fn send_reply<Request, Response>(
 *          &self,
 *          req: &Request,
 *      ) -> Result<Response, Error>
 *      where
 *          Request: Serialize + Schema,
 *          Response: Deserialize + Schema,
 *      {
 *          let buf = serialize_with_header(req)?;
 *          let (header, body) = self.send_reply_raw(&buf);
 *          check(&header)?;
 *          deser(body)
 *      }
 *      fn send_reply_raw<'a>(
 *          &self,
 *          req: &[u8],
 *      ) -> Result<(Header, &'a mut [u8]), Error>;
 * }
 *
 * // client interface looks like:
 * trait TwoServerClient {
 *      fn one_more_thing(&self, req: &u32) -> Result<bool, Error>;
 * }
 *
 * // This is extension trait shenanigans, not sure if I can do better.
 * impl<B: Backend> TwoServerClient for B {
 *      fn one_more_thing(&self, req: &u32) -> Result<bool, Error> {
 *          self.send_reply::<u32, bool>(req)
 *      }
 * }
 */

// macro_rules! endpoints {
//     ($(
//         $(#[cfg($meta:meta)])?
//         $name: ident:
//         $req:ty
//         =>
//         $resp:ty$(,)?
//     )*) => {{
//         const LIST: &[EndpointInfo] = &[$(
//             $(#[cfg($meta)])?
//             const {
//                 struct Boop {}
//                 impl Endpoint for Boop {
//                     const NAME: &'static str = stringify!($name);
//                     type Request = $req;
//                     type Response = $resp;
//                 }
//                 EndpointInfo::info::<Boop>()
//             },
//         )*];
//         LIST
//     }};
// }

// const ENDPOINTS: &[EndpointInfo] = endpoints!(
//     #[cfg(not(feature = "std"))] lol: u32 => u32,
//     lmao: u32 => u32,
//     // "lol": u32 => u32,
// );

// pub struct EndpointInfo {
//     pub name: &'static str,
//     pub key: Key,
// }

/////////////////////////////////////////////////////////
// EXAMPLES
/////////////////////////////////////////////////////////

interface! {
     mod: ops,
     | method            | request       | response      |
     | ------            | -------       | --------      |
     | mult_three        | u32           | u32           |
}

#[derive(Schema, Deserialize, Serialize)]
pub struct Str<'a>(&'a str);

#[derive(Schema, Deserialize, Serialize)]
pub struct Str2<'a, 'b>(&'a str, &'b str);

#[derive(Schema, Deserialize, Serialize)]
pub struct Fancy {
    a: u8,         // 1
    b: u16,        // 3
    c: u32,        // 5
    d: u64,        // 10
    e: (i8, i16),  // 4
    f: (i32, i64), // 15
    g: [bool; 8],  // 8
                   // ==
                   // 46 ✔️
}

#[cfg(not(feature = "std"))]
interface! {
     mod: basic,
     | method     | request | response     |
     | ------     | ------- | --------     |
     | mult_two   | u32     | u32          |
     | to_stringa | u32     | Str<'b>      |
     | to_stringb | Str<'a> | u32          |
     | billy      | Str<'a> | Str<'b>      |
     | to_stringd | Str<'a> | Str2<'b, 'c> |
}

#[cfg(feature = "std")]
interface! {
     mod: basic,
     | method     | request | response     |
     | ------     | ------- | --------     |
     | mult_two   | u32     | u32          |
     | to_stringa | u32     | String       |
     | to_stringb | String  | u32          |
     | billy      | Str<'a> | Str<'b>      |
     | to_stringd | String  | String       |
     | is_good    | Fancy   | bool         |
}

interface! {
     mod: two,
     | method         | request | response |
     | ------         | ------- | -------- |
     | one_more_thing | u32     | bool     |
}

compose_interfaces! {
     mod: composite,
     interfaces: [
         basic,
         two,
         ops,
     ]
}

pub struct ServerImpl;

impl basic::Server for ServerImpl {
    fn mult_two(&mut self, req: crate::Request<u32>) -> u32 {
        req.req * 2
    }

    fn to_stringa(&mut self, req: crate::Request<u32>) -> String {
        req.req.to_string()
    }

    fn to_stringb(&mut self, req: crate::Request<String>) -> u32 {
        req.req.len() as u32
    }

    fn to_stringd(&mut self, req: crate::Request<String>) -> String {
        req.req
    }

    fn billy<'a, 'b>(&mut self, req: crate::Request<Str<'a>>) -> Str<'b> {
        if req.req.0.len() > 5 {
            Str(":(")
        } else {
            Str(":)")
        }
    }

    fn is_good(&mut self, req: crate::Request<Fancy>) -> bool {
        let mut good = true;
        for g in req.req.g {
            good &= g;
        }
        good
    }
}

impl ops::Server for ServerImpl {
    fn mult_three(&mut self, req: crate::Request<u32>) -> u32 {
        req.req * 3
    }
}

impl two::Server for ServerImpl {
    fn one_more_thing(&mut self, req: crate::Request<u32>) -> bool {
        req.req.is_multiple_of(2)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    pub fn exercise() {
        let mut x = ServerImpl;
        let mut output = [0u8; 16];
        let mut input = [0u8; 16];

        let iused = postcard::to_slice(&200u32, &mut input).unwrap();
        let resp = <ServerImpl as composite::Server>::process_one(
            &mut x,
            Header {
                seqno: 123,
                version: 0,
                method: Method::Request,
                key: endpoint_key2::<u32, u32>("mult_two"),
            },
            iused,
            &mut output,
        )
        .unwrap();
        let actually: u32 = postcard::from_bytes(resp).unwrap();
        assert_eq!(actually, 400u32);
    }

    #[test]
    pub fn exercise_borrowed() {
        let mut x = ServerImpl;
        let mut output = [0u8; 16];
        let mut input = [0u8; 16];

        let iused = postcard::to_slice("boop", &mut input).unwrap();
        let resp = <ServerImpl as composite::Server>::process_one(
            &mut x,
            Header {
                seqno: 124,
                version: 0,
                method: Method::Request,
                key: endpoint_key2::<Str, Str>("billy"),
            },
            iused,
            &mut output,
        )
        .unwrap();
        let actually: Str<'_> = postcard::from_bytes(resp).unwrap();
        assert_eq!(actually.0, ":)");
    }

    #[test]
    pub fn sizes() {
        for s in composite::schemas::ALL_REQ_SCHEMAS {
            println!("{s:?} -> {:?}", s.max_size());
        }
        println!();
        for s in composite::schemas::ALL_RESP_SCHEMAS {
            println!("{s:?} -> {:?}", s.max_size());
        }
        println!();
        for k in composite::keys::ALL_KEYS {
            println!("{k:?}");
        }
        panic!();
    }
}

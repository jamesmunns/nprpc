// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use postcard_schema::{Schema, key::Key};
use serde::{Deserialize, Serialize};

/////////////////////////////////////////////////////////
// EXAMPLES
/////////////////////////////////////////////////////////

/// Internal macro for merging generated keylists, used for
/// `compose_interfaces!`
#[macro_export]
macro_rules! merge_keylists {
    ($($($segment:ident)::+$(,)?)*) => {{
        const TOTAL_LEN: usize = $crate::count_keys(
            &[$(
                $($segment)::+,
            )*]
        );
        const ALL_KEYS_MERGED: [postcard_schema::key::Key; TOTAL_LEN] = const {
            const ONE: postcard_schema::key::Key = unsafe {
                postcard_schema::key::Key::from_bytes([0u8; 8])
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
        const SLI: &[postcard_schema::key::Key] = &ALL_KEYS_MERGED;
        SLI
    }};
}

/// ```rust
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

            /// Calculated keys for all methods in this interface
            pub mod keys {
                #[allow(unused_imports)]
                use super::*;
                $(
                    #[allow(non_upper_case_globals)]
                    pub const $mthd: postcard_schema::key::Key = $crate::endpoint_key2::<$req_ty, $resp_ty>(stringify!($mthd));
                )*

                pub const ALL_KEYS: &[postcard_schema::key::Key] = &[
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
                            keys::$mthd => $crate::process::<
                                Self,
                                $req_ty,
                                $resp_ty,
                            >(hdr, body, output, <Self as Server>::$mthd),
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

/// ```rust
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
                    const MERGED_KEYS: &[postcard_schema::key::Key] = $crate::merge_keylists!(
                        $(
                            super::$intfc::keys::ALL_KEYS,
                        )*
                    );
                    const _: () = $crate::assert_unique(MERGED_KEYS);

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

pub struct Header {
    key: Key,
}
pub struct Request<T> {
    pub hdr: Header,
    pub req: T,
}

pub enum Error {
    Unknown,
}

pub fn process<'a, S: ?Sized, Q, R>(
    _hdr: Header,
    _body: &[u8],
    _out: &'a mut [u8],
    _apply: fn(&mut S, Request<Q>) -> R,
) -> Result<&'a mut [u8], Error> {
    // deserialize
    // let req = Request::new(
    //      hdr,
    //      req: deser(_body)?,
    // };
    // let resp = apply(S, req);
    // let used = ser(&resp)?;
    // Ok(used)
    todo!()
}

/////////////////////////////////////////////////////////
// CONST HELPERS
/////////////////////////////////////////////////////////

pub const fn assert_impls_schema<T: postcard_schema::Schema>() {}
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
    unsafe { Key::from_bytes(out) }
}

const fn count_keys(keys: &[&[Key]]) -> usize {
    let mut idx = 0;
    let mut ct = 0;
    while idx < keys.len() {
        ct += keys[idx].len();
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
 * // server interface looks like:
 * trait Server {
 *      // Wrapper type that asserts uniqueness and adds schema endpoints?
 *      const SCHEMAS: &'static [EndpointInfo] = ...;
 *      fn one_more_thing(&mut self, req: Request<u32>) -> bool;
 *      fn process_one<S: Server>(
 *           server: &mut S,
 *           hdr: Header,
 *           body: &[u8],
 *           output: &mut [u8],
 *      ) -> usize {
 *          // match style
 *          const one_more_thing: Key = Key::for<u32, bool>("one_more_thing");
 *          match hdr.key {
 *              one_more_thing => {
 *                  // TODO: make this an inner func that
 *                  let req: u32 = deser(body)?;
 *                  let req: Request<u32> = Request::new(&hdr, &req);
 *                  let resp = <S as Server>::one_more_thing(server, req)?;
 *                  serialize(output, &resp)?;
 *                  Ok(())
 *              }
 *          }
 *
 *          // search style
 *          fn handle<S: Server, Q: Deserialize, R: Serialize>(
 *              server: &mut S,
 *              hdr: Header,
 *              body: &[u8],
 *              output: &mut [u8],
 *              func: fn(&mut S, Request<Q>) -> Result<R, Error>,
 *          ) -> usize {
 *              let req: u32 = deser::<Q>(body)?;
 *              let req: Request<u32> = Request::new(&hdr, &req);
 *              let resp: R = <S as Server>::one_more_thing(server, req)?;
 *              let used = serialize(output, &resp)?;
 *              Ok(used)
 *          }
 *          const MAP: &[(Key, fn(&mut S, Header, body, output))] = &[
 *              (Key::for<u32, bool>("one_more_thing"), handle::<S, u32, bool>),
 *          ];
 *      }
 * }
 *
 * // In the server location, this is what the user actually implements
 * impl two::Server {
 *      fn one_more_thing(&mut self, req: Request<u32>) -> bool {
 *          // ...
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
     | billy      | String  | String       |
     | to_stringd | String  | String       |
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

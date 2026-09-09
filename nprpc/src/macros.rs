//! Declarative macros

use postcard_schema_ng::key::Key;

/// Check whether any duplicates are present in the given list.
///
/// Returns `false` if duplicates were found (NOT unique), otherwise returns
/// `true`.
pub const fn assert_unique(keys: &[Key]) -> bool {
    let mut i = 0;
    while i < keys.len() {
        let mut j = i + 1;
        let a = u64::from_le_bytes(keys[i].to_bytes());
        while j < keys.len() {
            let b = u64::from_le_bytes(keys[j].to_bytes());
            if a == b {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// # Interface Definition macro
///
/// This macro is used for defining an **interface**, a set of methods with
/// associated request and response types.
///
/// ## Lifetimes
///
/// We support two specific lifetimes when defining an interface:
///
/// * `'rqst`:
///     * For servers: this information is borrowed from the raw incoming
///       request message when deserializing.
///     * For clients: this information is borrowed from the user-passed request.
///     * This is the only lifetime that can be used for requests.
/// * `'resp`:
///     * For servers: this information is borrowed from the server itself.
///     * For clients: this information is borrowed from the raw incoming
///       response message when deserializing.
///     * This is the only lifetime that can be used for responses.
///
/// ## Example
///
/// ```rust
/// # #![allow(clippy::unexpected_cfgs)]
/// use nprpc::interface;
///
/// // This defines an interface. This interface is called "example".
/// // We use the `mod` keyword because we generate a Rust `mod` with
/// // the given name, containing various `trait`s and `const` metadata.
/// interface! {
///     /// Module level comments are supported, and will be places on the
///     /// generated module.
///     mod example {
///         /// Doc comments are also supported on the interface methods
///         ///
///         /// All interfaces take a single request type and return a single
///         /// response type. You can use `()` if you don't need one or either.
///         fn is_even(u32) -> bool;
///
///         /// `cfg` features are also supported. In this example, the
///         /// `mult_three` method is only present if the `"odd"` feature is
///         /// activated.
///         #[cfg(feature = "odd")]
///         fn mult_three(u32) -> u32;
///
///         /// We can also use `cfg` features to abstract over systems that do
///         /// or don't have a heap. We support "type punning", any type that
///         /// is string-shaped has the same schema, and therefore is
///         /// compatible, even if the systems hold them differently.
///         ///
///         /// On our no-std system, we will use a borrowed string with a
///         /// bounded max len of 3 elements, enough to convert the `u8` to
///         /// text. We can use the `'resp` lifetime to for borrowed responses.
///         #[cfg(not(feature = "std"))]
///         fn to_string(u8) -> postcard_schema_ng::max_len::MaxLenStr<'resp, 3>;
///
///         /// On a hosted machine, we might not want to borrow our data, and
///         /// instead directly heap-allocate the response instead. Notice how
///         /// We don't include the lifetime here, because the response will
///         /// be owned, and not borrowed.
///         ///
///         /// You can also use types like `heapless::Vec<u32, N>` (on no-std)
///         /// non-borrowed collections, and then use `std::vec::Vec<u32>` on
///         /// the hosted machine.
///         #[cfg(feature = "std")]
///         fn to_string(u8) -> postcard_schema_ng::max_len::MaxLenString<3>;
///
///         /// We can also borrow from the incoming message to get a borrowed
///         /// str out of the request, using the `'rqst` lifetime.
///         fn to_byte(postcard_schema_ng::max_len::MaxLenStr<'rqst, 3>) -> u8;
///     }
/// }
/// ```
#[macro_export]
macro_rules! interface {
    (
        $(#[doc = $mod_doc:literal])*
        mod $mod_name:ident {
            $(
                $(#[doc = $mthd_doc:literal])*
                $(#[cfg($mthd_cfg:meta)])*
                fn $mthd:ident($rqst_ty:ty) -> $resp_ty:ty;
            )*
        }
 ) => {
        #[doc = concat!("`", stringify!($mod_name), "` interface definition")]
        ///
        /// Generated interface definition. See the [`Server`] and [`Client`]
        /// traits for more information.
        ///
        #[doc = concat!("[`Server`]: ", stringify!($mod_name), "::Server")]
        #[doc = concat!("[`Client`]: ", stringify!($mod_name), "::Client")]
        pub mod $mod_name {
            #[allow(unused_imports)]
            use super::*;
            use $crate::{io::client::Backend, RequestRaw};

            /// The `methods` module contains metadata about each of the methods
            /// of an interface, and implementations of the `Method` trait.
            ///
            /// You don't usually need to use these items directly.
            pub mod methods {
                #[allow(unused_imports)]
                use super::*;

                $(
                    $(#[doc = $mthd_doc])*
                    $(#[cfg($mthd_cfg)])?
                    #[allow(non_camel_case_types)]
                    pub struct $mthd;

                    $(#[cfg($mthd_cfg)])?
                    impl $crate::interface::Method for $mthd {
                        type Request<'rqst> = $rqst_ty;
                        type Response<'resp> = $resp_ty;
                        const NAME: &'static str = concat!(stringify!($mod_name), "/", stringify!($mthd));
                    }
                )*
            }

            /// Calculated keys for all methods in this interface
            pub mod keys {
                #[allow(unused_imports)]
                use super::*;
                use $crate::__private::Key;

                /// All calculated [`Key`]s used for methods in this interface
                pub const ALL_KEYS: &[Key] = &[
                    $(
                        $(#[cfg($mthd_cfg)])?
                        <methods::$mthd as $crate::interface::Method>::KEY,
                    )*
                ];
            }

            /// The `info` module contains metadata about the interface itself.
            ///
            /// You don't usually need to use these items directly.
            pub mod info {
                #[allow(unused_imports)]
                use super::*;
                use $crate::interface::{MethodInfo, InterfaceInfo, Method};

                /// A list of [`MethodInfo`] for all methods of this interface
                const ALL_METHOD_INFOS: &[MethodInfo] = &[
                    $(
                        $(#[cfg($mthd_cfg)])?
                        <super::methods::$mthd as Method>::INFO,
                    )*
                ];

                /// Information about the interface, including the list of all methods
                /// and the max request/response body size (NOT including any headers!).
                pub const INTERFACE_INFO: InterfaceInfo = InterfaceInfo {
                    max_request_size: $crate::interface::rqst_body_max_buf_required(ALL_METHOD_INFOS),
                    max_response_size: $crate::interface::resp_body_max_buf_required(ALL_METHOD_INFOS),
                    methods: ALL_METHOD_INFOS,
                };
            }

            #[doc = concat!("The `", stringify!($mod_name), "` interface server trait")]
            pub trait Server {
                // Generate all of the user-filled method declarations
                $(
                    $(#[doc = $mthd_doc])*
                    $(#[cfg($mthd_cfg)])?
                    fn $mthd<'rqst, 'resp>(&'resp mut self, rqst: $crate::Request<$rqst_ty>) -> $resp_ty;
                )*

                /// This method is the prime dispatcher. It takes a processed header and raw body,
                /// and dispatches it to a method if there is a matching one, otherwise returning
                /// Err(Unknown) if the key didn't match.
                fn dispatch_one<'resp>(
                    &mut self,
                    req_raw: RequestRaw<'_>,
                    output: &'resp mut [u8],
                ) -> Result<&'resp [u8], $crate::ServerError> {
                    // This block ensures that there are no key collisions in all methods
                    const _: () = assert!(
                        $crate::macros::assert_unique(keys::ALL_KEYS),
                        concat!("key collision in interface `", stringify!($mod_name), "`"),
                    );

                    // Dispatch based on the received key. We trampoline through a monomorphized
                    // version of the `process_method
                    // _request` function, which handles the
                    // common deserialize, process, serialize portion of the code. This is
                    // stamped out on a per-method basis.
                    match req_raw.hedr.key {
                        $(
                            $(#[cfg($mthd_cfg)])?
                            <methods::$mthd as $crate::interface::Method>::KEY => {
                                $crate::io::server::process_method_request::<methods::$mthd, _>(
                                    req_raw,
                                    output,
                                    |rqst| <Self as Server>::$mthd(self, rqst)
                                )
                            }
                        )*

                        // None of the keys matched, return an error.
                        _ => Err($crate::ServerError::UnknownMethod),
                    }
                }

                fn serve_one<B: $crate::io::server::Backend>(
                    &mut self,
                    backend: &mut B,
                ) -> Result<(), $crate::io::server::ServerIoError<<B::Io as $crate::io::server::Io>::Error>> {
                    $crate::io::server::serve_one_with_dispatcher::<B>(
                        backend,
                        |rqst_raw, resp_buf| self.dispatch_one(rqst_raw, resp_buf)
                    )
                }
            }

            #[doc = concat!("The `", stringify!($mod_name), "` client trait")]
            ///
            /// The `Client` trait is an extension trait that is implemented for
            /// all [`Backend`] implementations.
            pub trait Client: Backend {
                $(
                    $(#[doc = $mthd_doc])*
                    $(#[cfg($mthd_cfg)])?
                    #[allow(clippy::ptr_arg)]
                    fn $mthd<'rqst, 'resp>(&'resp mut self, rqst: &'rqst $rqst_ty)
                        -> Result<
                            $crate::Response<$resp_ty>,
                            $crate::ClientIoError<<Self::Io as $crate::io::client::Io>::Error>
                        > {
                            self.send_then_receive_typed_frames::<methods::$mthd>(
                                rqst,
                            )
                        }
                )*
            }

            impl<T: Backend> Client for T {}
        }
    };
}

/// Macro to combine multiple interfaces into a single composite interface.
///
/// Produces the same module structure as the [`interface!`] macro. This does
/// NOT include the `Client` trait, it is still necessary to pull individual
/// Client traits.
///
/// ## Example
///
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
            pub mod keys {
                use $crate::__private::Key;
                use super::info::ALL_METHOD_INFOS;
                const LEN: usize = ALL_METHOD_INFOS.len();
                pub const ALL_KEYS: &[Key] = &$crate::interface::extract_keys::<LEN>(ALL_METHOD_INFOS);
            }

            pub mod info {
                #[allow(unused_imports)]
                use super::*;
                use $crate::interface::{MethodInfo, InterfaceInfo};
                pub const ALL_METHOD_INFOS: &[MethodInfo] = {
                    const SETS: &[&[MethodInfo]] = &[
                        $(
                            $($segment)::+::info::INTERFACE_INFO.methods,
                        )*
                    ];
                    const N: usize = $crate::interface::total_len(SETS);
                    const ARR: [MethodInfo; N] = $crate::interface::flatten(SETS);
                    &ARR
                };

                pub const INTERFACE_INFO: InterfaceInfo = InterfaceInfo {
                    max_request_size: $crate::interface::rqst_body_max_buf_required(ALL_METHOD_INFOS),
                    max_response_size: $crate::interface::resp_body_max_buf_required(ALL_METHOD_INFOS),
                    methods: ALL_METHOD_INFOS,
                };
            }

            pub trait Server {
                fn dispatch_one<'buf>(
                    &mut self,
                    req_raw: $crate::RequestRaw<'_>,
                    output: &'buf mut [u8],
                ) -> Result<&'buf [u8], $crate::ServerError>;

                fn serve_one<B: $crate::io::server::Backend>(
                    &mut self,
                    backend: &mut B,
                ) -> Result<(), $crate::io::server::ServerIoError<<B::Io as $crate::io::server::Io>::Error>> {
                    $crate::io::server::serve_one_with_dispatcher::<B>(
                        backend,
                        |rqst_raw, resp_buf| self.dispatch_one(rqst_raw, resp_buf)
                    )
                }
            }

            // TODO: Remove this blanket impl (or make optional) to allow for
            // proxying here?
            //
            // UPDATE: I think we keep this as-is, because it's only implemented
            // for T's that implement ALL sub-server traits.
            //
            // If we have three interfaces A, B, C; and the server implements
            // A and B as a server, and wants to proxy C, it *won't* automatically
            // get this impl, and still allows for a separate `declare proxies`
            // macro that looks like:
            //
            // ```rust
            // server_impl! {
            //      impl composite::Server for ServerImpl {
            //          // These are handled by ServerImpl's normal impls, and
            //          // calls `dispatch_one` like we do in this macro below
            //          crate::a => self,
            //          crate::b => self,
            //
            //          // This requires that ServerImpl holds a `c_client` that
            //          // implements the `c::Client` trait, and calls some kind
            //          // of raw proxy method on the client trait that skips
            //          // the serialization/deserialization steps.
            //          crate::c => proxy(self.c_client),
            //      }
            // }
            // ```
            impl<T> Server for T
            where
                $(T: $($segment)::+::Server,)*
            {
                fn dispatch_one<'buf>(
                    &mut self,
                    req_raw: $crate::RequestRaw<'_>,
                    output: &'buf mut [u8],
                ) -> Result<&'buf [u8], $crate::ServerError> {
                    // Check all the merged keys to make sure that none of the composed
                    // methods have a collision
                    const _: () = assert!(
                        $crate::macros::assert_unique(keys::ALL_KEYS),
                        concat!("key collision in composite interface `", stringify!($mod_name), "`"),
                    );

                    $(
                        // We know that all keys are unique, so instead of pre-checking whether a
                        // sub-interface can handle a request, we just give it to each sub-interface
                        // one at a time until one responds with a non-Unknown result, or we have
                        // exhausted all outcomes.
                        //
                        // TODO: j/k, that causes borrow errors. Turns out this is NLL problem case #3:
                        // https://rust-lang.github.io/rfcs/2094-nll.html#problem-case-3-conditional-control-flow-across-functions
                        // Revisit this once Polonius lands someday.
                        //
                        // let res = <Self as $($segment)::+::Server>::dispatch_one(self, req_raw, output);
                        // if res != Err($crate::ServerError::UnknownMethod) {
                        //     return res;
                        // }
                        //
                        // For now, we end up checking the keys twice, here in a `contains` check, and then
                        // inside dispatch in the form of the `match` statement. Slight perf bummer, but
                        // at least on hubris the number of keys should be reasonably small (and we early
                        // return if keys aren't similar).
                        if $($segment)::+::keys::ALL_KEYS.contains(&req_raw.hedr.key) {
                            return <Self as $($segment)::+::Server>::dispatch_one(self, req_raw, output);
                        }
                    )*

                    Err($crate::ServerError::UnknownMethod)
                }
            }
        }
    };
}

/// Defines a buffer type for a given interface
///
/// This defines a struct with two fields that are `[u8; N]` arrays that are of
/// sufficient size to hold the largest single request and largest single
/// response for the given interface, including headers.
///
/// This generated struct also implements the [`Storage`](crate::io::Storage)
/// trait.
///
/// ```rust
/// use nprpc::{interface, autobuffer};
///
/// interface! {
///     mod example {
///         fn method(u32) -> u64;
///     }
/// }
///
/// autobuffer!(ApiBufs, example);
///
/// let bufs = ApiBufs::new();
/// assert_eq!(core::mem::size_of_val(&bufs.rqst_buf), 18);
/// assert_eq!(core::mem::size_of_val(&bufs.resp_buf), 23);
/// ```
#[macro_export]
macro_rules! autobuffer {
    ($name:ident, $($segment:ident)::+) => {
        #[doc = concat!("Automatically sized buffers for the `", stringify!($($segment)::+), "` interface")]
        pub struct $name {
            pub rqst_buf: [u8; Self::_RQST_SIZE],
            pub resp_buf: [u8; Self::_RESP_SIZE],
        }

        impl $name {
            const _HDR_SIZE: usize = <$crate::wire::Header as $crate::__private::Schema>::SCHEMA.max_size()
            .expect("Unable to automatically size buffer. \
                Header doesn't have a max size.");
            const _RQST_BODY_SIZE: usize = $($segment)::+::info::INTERFACE_INFO.max_request_size
                .expect("Unable to automatically size buffer. \
                    One or more request types don't have a max size.");
            const _RESP_BODY_SIZE: usize = $($segment)::+::info::INTERFACE_INFO.max_response_size
                .expect("Unable to automatically size buffer. \
                    One or more response types don't have a max size.");
            const _RQST_SIZE: usize = Self::_HDR_SIZE + Self::_RQST_BODY_SIZE;
            const _RESP_SIZE: usize = Self::_HDR_SIZE + Self::_RESP_BODY_SIZE;

            pub const fn new() -> Self {
                Self {
                    rqst_buf: [0u8; Self::_RQST_SIZE],
                    resp_buf: [0u8; Self::_RESP_SIZE],
                }
            }
        }

        impl $crate::io::Storage for $name {
            fn buffers(&mut self) -> $crate::io::StorageView<'_> {
                let Self { rqst_buf, resp_buf } = self;
                $crate::io::StorageView { rqst_buf, resp_buf }
            }
        }
    };
}

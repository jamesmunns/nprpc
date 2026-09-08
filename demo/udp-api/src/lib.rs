#![cfg_attr(not(feature = "std"), no_std)]

use nprpc::{compose_interfaces, interface};

/////////////////////////////////////////////////////
// API types
/////////////////////////////////////////////////////
#[cfg(not(feature = "std"))]
pub use postcard_schema_ng::max_len::MaxLenStr;
#[cfg(feature = "std")]
pub use postcard_schema_ng::max_len::MaxLenString;

/////////////////////////////////////////////////////
// Interface definitions
/////////////////////////////////////////////////////

interface! {
    mod hello {
        fn loopback(u32) -> u32;
    }
}

interface! {
    mod kv {
        #[cfg(feature = "std")]
        fn set_name(MaxLenString<32>) -> ();
        #[cfg(not(feature = "std"))]
        fn set_name(MaxLenStr<'req, 32>) -> ();
        #[cfg(feature = "std")]
        fn get_name(()) -> Option<MaxLenString<32>>;
        #[cfg(not(feature = "std"))]
        fn get_name(()) -> Option<MaxLenStr<'resp, 32>>;
    }
}

compose_interfaces! {
    mod: composite,
    interfaces: [
        crate::hello,
        crate::kv,
    ]
}

// TODO: We could make this a builtin part of nprpc? But it's also pretty easy
// to opt-in to, so maybe we keep it more as a "recipe" than a built-in.
#[cfg(test)]
mod test {
    use insta::assert_debug_snapshot;

    #[test]
    fn snapshot_schemas() {
        assert_debug_snapshot!(crate::composite::info::INTERFACE_INFO);
    }
}

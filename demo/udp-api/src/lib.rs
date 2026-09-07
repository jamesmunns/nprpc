use nprpc::{compose_interfaces, interface};

/////////////////////////////////////////////////////
// API types
/////////////////////////////////////////////////////
use postcard_schema_ng::max_len::MaxLenString;

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
        fn set_name(MaxLenString<32>) -> ();
        fn get_name(()) -> Option<MaxLenString<32>>;
    }
}

compose_interfaces! {
    mod: composite,
    interfaces: [
        crate::hello,
        crate::kv,
    ]
}

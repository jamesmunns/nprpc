//! Tests for flattening interfaces, and decoding them again

use std::collections::BTreeMap;

use postcard_schema_ng::{
    Schema,
    max_len::{MaxLenStr, MaxLenString},
    schema::owned::OwnedDataModelType,
};
use serde::{Deserialize, Serialize};

use nprpc::{
    compose_interfaces, flat::DecodeError, flat::FLAT_VERSION, flat::FlatNode,
    flat::OwnedInterfaceInfo, flat_interface, interface, interface::InterfaceInfo,
};

#[derive(Serialize, Deserialize, Schema)]
pub struct Unit;

#[derive(Serialize, Deserialize, Schema)]
pub struct Newtype(pub i16);

#[derive(Serialize, Deserialize, Schema)]
pub struct TupleStruct(pub u8, pub Option<char>);

#[derive(Serialize, Deserialize, Schema)]
pub struct Fields {
    pub a: u64,
    pub b: [u8; 3],
    pub c: (bool, f32),
}

#[derive(Serialize, Deserialize, Schema)]
pub enum Everything {
    Unit,
    Newtype(Fields),
    Tuple(i32, Newtype),
    Struct { x: TupleStruct, y: Unit },
}

interface! {
    mod iface_one {
        fn nothing(()) -> ();
        fn every(Everything) -> Option<Everything>;
        fn strs(MaxLenStr<'rqst, 8>) -> MaxLenString<16>;
        fn unbounded(Vec<u128>) -> BTreeMap<String, Vec<u8>>;
    }
}

interface! {
    mod iface_two {
        fn wide(i128) -> (u16, i64, f64);
    }
}

compose_interfaces! {
    mod: composite,
    interfaces: [crate::iface_one, crate::iface_two]
}

flat_interface! {
    static ONE = iface_one;
}

flat_interface! {
    #[used]
    pub static COMPOSITE = composite;
}

/// What we expect decoding a flattened interface to produce
fn expected(name: &str, info: &InterfaceInfo) -> OwnedInterfaceInfo {
    OwnedInterfaceInfo {
        name: name.to_string(),
        max_request_size: info.max_request_size,
        max_response_size: info.max_response_size,
        methods: info
            .methods
            .iter()
            .map(|m| nprpc::flat::OwnedMethodInfo {
                name: m.name.to_string(),
                key: m.key,
                req_schema: OwnedDataModelType::from(m.req_schema),
                resp_schema: OwnedDataModelType::from(m.resp_schema),
            })
            .collect(),
    }
}

#[test]
fn round_trip() {
    assert_eq!(
        ONE.as_ref().decode().unwrap(),
        expected("iface_one", &iface_one::info::INTERFACE_INFO),
    );
    assert_eq!(
        COMPOSITE.as_ref().decode().unwrap(),
        expected("composite", &composite::info::INTERFACE_INFO),
    );
}

#[test]
fn layout() {
    assert_eq!(ONE.version, FLAT_VERSION);
    assert_eq!(ONE.methods.len(), 4);
    assert_eq!(COMPOSITE.methods.len(), 5);

    // `nothing` takes and returns `()`, which is the same node
    let nothing = ONE.methods[0];
    assert_eq!(nothing.rqst, nothing.resp);
    assert_eq!(ONE.nodes[nothing.rqst as usize], FlatNode::Unit);

    // `every` returns `Option<Everything>`, which refers to the same
    // `Everything` node that it takes
    let every = ONE.methods[1];
    assert_eq!(
        ONE.nodes[every.resp as usize],
        FlatNode::Option { inner: every.rqst }
    );

    // Names are stored inline, interface name first.
    let name = &ONE.names[..ONE.name.len as usize];
    assert_eq!(name, b"iface_one");
    let m = ONE.methods[2].name;
    let m = &ONE.names[m.offset as usize..][..m.len as usize];
    assert_eq!(m, b"iface_one/strs");

    // Max sizes are carried through, and unbounded types don't have one.
    assert_eq!(ONE.max_request_size, None);
}

#[test]
fn unique() {
    // Every node is unique
    for (i, a) in COMPOSITE.nodes.iter().enumerate() {
        for b in &COMPOSITE.nodes[i + 1..] {
            assert_ne!(a, b);
        }
    }

    // Names are only stored once, e.g. the `Unit` struct and variant
    let count = |s: &[u8]| COMPOSITE.names.windows(s.len()).filter(|w| *w == s).count();
    assert_eq!(count(b"Unit"), 1);

    // Everything is smaller than without de-duplication
    let scratch =
        nprpc::flat::flat_scratch(composite::info::NAME, &composite::info::INTERFACE_INFO);
    assert!(COMPOSITE.nodes.len() < scratch.nodes);
    assert!(COMPOSITE.children.len() < scratch.children / 2);
    assert!(COMPOSITE.names.len() < scratch.names);
}

#[test]
fn bad_input() {
    let mut bad = ONE;
    bad.version += 1;
    assert_eq!(
        bad.as_ref().decode(),
        Err(DecodeError::UnsupportedVersion(FLAT_VERSION + 1))
    );

    // A method pointing past the end of the nodes
    let mut bad = ONE;
    bad.methods[0].rqst = ONE.nodes.len() as u32;
    assert_eq!(
        bad.as_ref().decode(),
        Err(DecodeError::MissingNode(ONE.nodes.len()))
    );

    // A method pointing at a non-type node
    let mut bad = ONE;
    let unit = bad.methods[0].rqst as usize;
    bad.nodes[unit] = FlatNode::DataUnit;
    assert!(matches!(
        bad.as_ref().decode(),
        Err(DecodeError::UnexpectedNode { .. })
    ));

    // A node referring to itself, which would loop forever
    let mut bad = ONE;
    let every = bad.methods[1].resp;
    bad.nodes[every as usize] = FlatNode::Option { inner: every };
    assert_eq!(
        bad.as_ref().decode(),
        Err(DecodeError::BadReference {
            parent: every as usize,
            child: every as usize,
        })
    );

    // A span past the end of the children
    let mut bad = ONE;
    let (idx, elems) = bad
        .nodes
        .iter()
        .enumerate()
        .find_map(|(i, n)| match n {
            FlatNode::DataStruct { fields } => Some((i, *fields)),
            _ => None,
        })
        .unwrap();
    let long = nprpc::flat::Span {
        start: elems.start,
        len: ONE.children.len() as u32,
    };
    bad.nodes[idx] = FlatNode::DataStruct { fields: long };
    assert_eq!(bad.as_ref().decode(), Err(DecodeError::BadSpan(long)));
}

//! Flattened and de-duplicated interface schemas
//!
//! [`InterfaceInfo`] and the [`DataModelType`]s it contains are a graph of
//! `&'static` references. That's convenient to work with, but means that
//! placing an `InterfaceInfo` in a `static` also places everything it refers
//! to in `.rodata`, even if the `static` itself is placed in a non-loaded
//! (debug only) linker section.
//!
//! A [`FlatInterface`] contains the same information as an `InterfaceInfo`,
//! but with no references. Each part of the [`FlatInterface`] is intern'd into
//! a de-duplicated array of parts: methods, nodes, children, and names.
//!
//! Definitions of methods and types are built out of indicies of the intern'd
//! parts.
//!
//! `FlatInterface`s are built at compile time with the [`flat_interface!`]
//! macro, and are intended to be read out of a binary by a debugger.
//!
//! ## Building
//!
//! Because we are building at compile time, we need to Do Trickery to obtain
//! right-sized arrays in const context. We use a mix of decl macros and const
//! fns to achieve this. This is very similar to how `postcard-rpc` tackled
//! similar problems. This module holds the const-fns, see the module
//! [`macros`](crate::macros) for the macro parts.
//!
//! Building works using the following main `const fn`s in this order:
//!
//! 1. [`flat_scratch()`] counts nodes, children, and name bytes without any
//!    de-duplication, giving an upper bound on how much space is needed.
//! 2. [`FlatScratch::new()`] builds the de-duplicated interface into scratch
//!    space of that (max) size, which can then be obtained using
//!    [`FlatScratch::lengths()`] which reports how much of the maxmimum size was
//!    actually used when de-duplicating.
//! 3. [`FlatInterface::new()`] copies the deduplicated items (which take up
//!    a partial slice of the max-size array) into the right-sized array.
//!
//! [`flat_interface!`]: crate::flat_interface

use postcard_schema_ng::schema::{Data, DataModelType, NamedField, Variant};

use crate::interface::{InterfaceInfo, MethodInfo};

/// Current [`FlatInterface`] layout version
///
/// Incremented whenever the layout of the flat types changes.
pub const FLAT_VERSION: u32 = 1;

////////////////////////////////////////////////////////////////////////////////
// STAGE ONE - `flat_scrach()`
//
// Calculate the upper bounds of how large our buffers will need to be,
// assuming there is absolutely no repetition of any counted part.
////////////////////////////////////////////////////////////////////////////////

/// Counts the space needed to flatten types, without de-duplication.
struct Counter {
    nodes: usize,
    children: usize,
    names: usize,
}

impl Counter {
    /// Count a given type, recursively.
    const fn count_ty(&mut self, ty: &DataModelType) {
        self.nodes += 1;

        // If we are a type that contains types, recurse until we reach a leaf
        // node (with no children).
        match ty {
            DataModelType::Option(inner) => self.count_ty(inner),
            DataModelType::Seq { element, .. } => self.count_ty(element),
            DataModelType::Tuple(tys) => self.count_tys(tys),
            DataModelType::Map { key, val, .. } => {
                self.count_ty(key);
                self.count_ty(val);
            }
            DataModelType::Struct { name, data } => {
                self.names += name.len();
                self.count_data(data);
            }
            DataModelType::Enum { name, variants } => {
                self.names += name.len();
                self.children += variants.len();
                let mut i = 0;
                while i < variants.len() {
                    self.nodes += 1;
                    self.names += variants[i].name.len();
                    self.count_data(&variants[i].data);
                    i += 1;
                }
            }
            _ => {}
        }
    }

    /// Count all of the given types, recursively.
    const fn count_tys(&mut self, tys: &[&DataModelType]) {
        self.children += tys.len();
        let mut i = 0;
        while i < tys.len() {
            self.count_ty(tys[i]);
            i += 1;
        }
    }

    /// Count the types of an enum variant, recursively.
    const fn count_data(&mut self, data: &Data) {
        self.nodes += 1;
        match data {
            Data::Unit => {}
            Data::Newtype(inner) => self.count_ty(inner),
            Data::Tuple(tys) => self.count_tys(tys),
            Data::Struct(fields) => {
                self.children += fields.len();
                let mut i = 0;
                while i < fields.len() {
                    self.nodes += 1;
                    self.names += fields[i].name.len();
                    self.count_ty(fields[i].ty);
                    i += 1;
                }
            }
        }
    }
}

/// The array lengths used when building a [`FlatInterface`]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlatLengths {
    pub methods: usize,
    pub nodes: usize,
    pub children: usize,
    pub names: usize,
}

/// Calculates how much scratch space is needed to flatten an interface.
///
/// This is an upper bound, assuming that nothing can be de-duplicated. The
/// scratch space for children is doubled, as half of it is used as a stack
/// while building.
pub const fn flat_scratch(name: &str, info: &InterfaceInfo) -> FlatLengths {
    let mut c = Counter {
        nodes: 0,
        children: 0,
        names: name.len(),
    };
    let mut i = 0;
    while i < info.methods.len() {
        let m = &info.methods[i];
        c.names += m.name.len();
        c.count_ty(m.req_schema);
        c.count_ty(m.resp_schema);
        i += 1;
    }
    FlatLengths {
        methods: info.methods.len(),
        nodes: c.nodes,
        children: 2 * c.children,
        names: c.names,
    }
}

////////////////////////////////////////////////////////////////////////////////
// STAGE TWO - `FlatScratch`
//
// Does the intern-ing, using buffers of the max capacity calculated by
// `flat_scratch` above.
////////////////////////////////////////////////////////////////////////////////

/// A name, stored in [`FlatInterface::names`]
///
/// Names are always UTF-8.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NameRef {
    /// Offset of the first byte of the name
    pub offset: u32,
    /// Length of the name in bytes
    pub len: u32,
}

/// A flattened [`MethodInfo`]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlatMethod {
    /// The method's name, including the interface name, e.g. `iface/method`
    pub name: NameRef,
    /// The method's key
    pub key: [u8; 8],
    /// Index of the request type's [`FlatNode`]
    pub rqst: u32,
    /// Index of the response type's [`FlatNode`]
    pub resp: u32,
}

/// A list of node indices, stored in [`FlatInterface::children`]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// Offset of the first index
    pub start: u32,
    /// Number of indices
    pub len: u32,
}

/// A single node of a flattened [`DataModelType`]
///
/// See the [module level docs](self) for what each node's children are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlatNode {
    Bool,
    I8,
    U8,
    I16,
    I32,
    I64,
    I128,
    U16,
    U32,
    U64,
    U128,
    Usize,
    Isize,
    F32,
    F64,
    Char,
    String {
        max_len: Option<u32>,
    },
    ByteArray {
        max_len: Option<u32>,
    },
    Option {
        inner: u32,
    },
    Unit,
    Seq {
        element: u32,
        max_len: Option<u32>,
    },
    Tuple {
        elems: Span,
    },
    Map {
        key: u32,
        val: u32,
        max_len: Option<u32>,
    },
    Struct {
        name: NameRef,
        data: u32,
    },
    Enum {
        name: NameRef,
        variants: Span,
    },
    Schema,
    Variant {
        name: NameRef,
        data: u32,
    },
    Field {
        name: NameRef,
        ty: u32,
    },
    DataUnit,
    DataNewtype {
        inner: u32,
    },
    DataTuple {
        elems: Span,
    },
    DataStruct {
        fields: Span,
    },
}

/// Writes de-duplicated nodes, children, and names into scratch space.
///
/// `children` is used from both ends: [`Span`]s are written from the start,
/// and the end is used as a stack of node indices, while collecting the nodes
/// of a span.
struct Writer<'a> {
    nodes: &'a mut [FlatNode],
    n_nodes: usize,
    children: &'a mut [u32],
    n_children: usize,
    /// The bottom of the stack at the end of `children`
    stack: usize,
    names: &'a mut [u8],
    n_names: usize,
}

impl<'a> Writer<'a> {
    const fn new(nodes: &'a mut [FlatNode], children: &'a mut [u32], names: &'a mut [u8]) -> Self {
        Self {
            stack: children.len(),
            nodes,
            n_nodes: 0,
            children,
            n_children: 0,
            names,
            n_names: 0,
        }
    }

    /// Writes all methods, returning the interface name.
    ///
    /// Methods are written to `methods` if it's long enough.
    const fn intern_interface(
        &mut self,
        name: &str,
        info: &InterfaceInfo,
        methods: &mut [FlatMethod],
    ) -> NameRef {
        let name = self.intern_name(name);
        let mut i = 0;
        while i < info.methods.len() {
            let m = self.intern_method(&info.methods[i]);
            if i < methods.len() {
                methods[i] = m;
            }
            i += 1;
        }
        name
    }

    const fn intern_method(&mut self, m: &MethodInfo) -> FlatMethod {
        FlatMethod {
            name: self.intern_name(m.name),
            key: m.key.to_bytes(),
            rqst: self.intern_ty(m.req_schema),
            resp: self.intern_ty(m.resp_schema),
        }
    }

    /// Returns the index of `node`, adding it if it isn't already present.
    const fn intern_node(&mut self, node: FlatNode) -> u32 {
        let mut i = 0;
        while i < self.n_nodes {
            if nodes_eq(&node, &self.nodes[i]) {
                return to_u32(i);
            }
            i += 1;
        }
        assert!(self.n_nodes < self.nodes.len(), "out of node space");
        self.nodes[self.n_nodes] = node;
        self.n_nodes += 1;
        to_u32(self.n_nodes - 1)
    }

    /// Returns a reference to `name`, adding it if it isn't already present.
    ///
    /// Any matching run of bytes is used, even if it's part of another name.
    const fn intern_name(&mut self, name: &str) -> NameRef {
        let bytes = name.as_bytes();
        let mut start = 0;
        while start + bytes.len() <= self.n_names {
            let mut i = 0;
            while i < bytes.len() && self.names[start + i] == bytes[i] {
                i += 1;
            }
            if i == bytes.len() {
                return NameRef {
                    offset: to_u32(start),
                    len: to_u32(bytes.len()),
                };
            }
            start += 1;
        }
        assert!(
            self.n_names + bytes.len() <= self.names.len(),
            "out of name space"
        );
        let mut i = 0;
        while i < bytes.len() {
            self.names[self.n_names + i] = bytes[i];
            i += 1;
        }
        self.n_names += bytes.len();
        NameRef {
            offset: to_u32(self.n_names - bytes.len()),
            len: to_u32(bytes.len()),
        }
    }

    /// Pushes a node index onto the stack, for a span being collected.
    const fn push_node(&mut self, idx: u32) {
        assert!(self.stack > self.n_children, "out of children space");
        self.stack -= 1;
        self.children[self.stack] = idx;
    }

    /// Pops the indices pushed since the stack was at `base`, returning a
    /// span containing them (in the order they were pushed).
    ///
    /// An existing span is used if it contains the same indices.
    const fn pop_span_since(&mut self, base: usize) -> Span {
        // The stack grows down, so pushed[k] is `self.children[base - 1 - k]`
        let len = base - self.stack;
        let mut start = 0;
        let found = loop {
            if start + len > self.n_children {
                break None;
            }
            let mut k = 0;
            while k < len && self.children[start + k] == self.children[base - 1 - k] {
                k += 1;
            }
            if k == len {
                break Some(start);
            }
            start += 1;
        };
        let start = match found {
            Some(start) => start,
            None => {
                // `stack <= base - len`, so this can't overwrite the stack
                // before it's read
                assert!(self.n_children + len <= self.stack, "out of children space");
                let mut k = 0;
                while k < len {
                    self.children[self.n_children + k] = self.children[base - 1 - k];
                    k += 1;
                }
                self.n_children += len;
                self.n_children - len
            }
        };
        self.stack = base;
        Span {
            start: to_u32(start),
            len: to_u32(len),
        }
    }

    /// Writes a type and its children, returning the type's node index.
    const fn intern_ty(&mut self, ty: &DataModelType) -> u32 {
        let node = match ty {
            DataModelType::Bool => FlatNode::Bool,
            DataModelType::I8 => FlatNode::I8,
            DataModelType::U8 => FlatNode::U8,
            DataModelType::I16 => FlatNode::I16,
            DataModelType::I32 => FlatNode::I32,
            DataModelType::I64 => FlatNode::I64,
            DataModelType::I128 => FlatNode::I128,
            DataModelType::U16 => FlatNode::U16,
            DataModelType::U32 => FlatNode::U32,
            DataModelType::U64 => FlatNode::U64,
            DataModelType::U128 => FlatNode::U128,
            DataModelType::Usize => FlatNode::Usize,
            DataModelType::Isize => FlatNode::Isize,
            DataModelType::F32 => FlatNode::F32,
            DataModelType::F64 => FlatNode::F64,
            DataModelType::Char => FlatNode::Char,
            DataModelType::String { max_len } => FlatNode::String {
                max_len: to_u32_opt(*max_len),
            },
            DataModelType::ByteArray { max_len } => FlatNode::ByteArray {
                max_len: to_u32_opt(*max_len),
            },
            DataModelType::Option(inner) => FlatNode::Option {
                inner: self.intern_ty(inner),
            },
            DataModelType::Unit => FlatNode::Unit,
            DataModelType::Seq { element, max_len } => FlatNode::Seq {
                element: self.intern_ty(element),
                max_len: to_u32_opt(*max_len),
            },
            DataModelType::Tuple(tys) => FlatNode::Tuple {
                elems: self.intern_tys(tys),
            },
            DataModelType::Map { key, val, max_len } => FlatNode::Map {
                key: self.intern_ty(key),
                val: self.intern_ty(val),
                max_len: to_u32_opt(*max_len),
            },
            DataModelType::Struct { name, data } => FlatNode::Struct {
                name: self.intern_name(name),
                data: self.intern_data(data),
            },
            DataModelType::Enum { name, variants } => {
                let name = self.intern_name(name);
                let base = self.stack;
                let mut i = 0;
                while i < variants.len() {
                    let idx = self.intern_variant(variants[i]);
                    self.push_node(idx);
                    i += 1;
                }
                FlatNode::Enum {
                    name,
                    variants: self.pop_span_since(base),
                }
            }
            DataModelType::Schema => FlatNode::Schema,
        };
        self.intern_node(node)
    }

    const fn intern_tys(&mut self, tys: &[&DataModelType]) -> Span {
        let base = self.stack;
        let mut i = 0;
        while i < tys.len() {
            let idx = self.intern_ty(tys[i]);
            self.push_node(idx);
            i += 1;
        }
        self.pop_span_since(base)
    }

    const fn intern_variant(&mut self, var: &Variant) -> u32 {
        let node = FlatNode::Variant {
            name: self.intern_name(var.name),
            data: self.intern_data(&var.data),
        };
        self.intern_node(node)
    }

    const fn intern_data(&mut self, data: &Data) -> u32 {
        let node = match data {
            Data::Unit => FlatNode::DataUnit,
            Data::Newtype(inner) => FlatNode::DataNewtype {
                inner: self.intern_ty(inner),
            },
            Data::Tuple(tys) => FlatNode::DataTuple {
                elems: self.intern_tys(tys),
            },
            Data::Struct(fields) => {
                let base = self.stack;
                let mut i = 0;
                while i < fields.len() {
                    let idx = self.intern_field(fields[i]);
                    self.push_node(idx);
                    i += 1;
                }
                FlatNode::DataStruct {
                    fields: self.pop_span_since(base),
                }
            }
        };
        self.intern_node(node)
    }

    const fn intern_field(&mut self, field: &NamedField) -> u32 {
        let node = FlatNode::Field {
            name: self.intern_name(field.name),
            ty: self.intern_ty(field.ty),
        };
        self.intern_node(node)
    }
}

/// A de-duplicated interface, built into scratch space
///
/// The const parameters are the scratch space to use, from [`flat_scratch()`].
/// Only the start of each array is used, see [`FlatScratch::lengths()`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlatScratch<
    const METHODS: usize,
    const S_NODES: usize,
    const S_CHILDREN: usize,
    const S_NAMES: usize,
> {
    name: NameRef,
    max_request_size: Option<u32>,
    max_response_size: Option<u32>,
    methods: [FlatMethod; METHODS],
    nodes: [FlatNode; S_NODES],
    n_nodes: usize,
    children: [u32; S_CHILDREN],
    n_children: usize,
    names: [u8; S_NAMES],
    n_names: usize,
}

impl<const METHODS: usize, const S_NODES: usize, const S_CHILDREN: usize, const S_NAMES: usize>
    FlatScratch<METHODS, S_NODES, S_CHILDREN, S_NAMES>
{
    /// Flattens and de-duplicates an interface into scratch space.
    pub const fn new(name: &str, info: &InterfaceInfo) -> Self {
        const EMPTY: FlatMethod = FlatMethod {
            name: NameRef { offset: 0, len: 0 },
            key: [0; 8],
            rqst: 0,
            resp: 0,
        };
        assert!(info.methods.len() == METHODS, "wrong method count");
        let mut out = Self {
            name: NameRef { offset: 0, len: 0 },
            max_request_size: to_u32_opt(info.max_request_size),
            max_response_size: to_u32_opt(info.max_response_size),
            methods: [EMPTY; METHODS],
            nodes: [FlatNode::Unit; S_NODES],
            n_nodes: 0,
            children: [0; S_CHILDREN],
            n_children: 0,
            names: [0; S_NAMES],
            n_names: 0,
        };
        let mut w = Writer::new(&mut out.nodes, &mut out.children, &mut out.names);
        let name = w.intern_interface(name, info, &mut out.methods);
        let (n_nodes, n_children, n_names) = (w.n_nodes, w.n_children, w.n_names);
        out.name = name;
        out.n_nodes = n_nodes;
        out.n_children = n_children;
        out.n_names = n_names;
        out
    }

    /// The exact lengths of the arrays needed to hold this interface.
    pub const fn lengths(&self) -> FlatLengths {
        FlatLengths {
            methods: METHODS,
            nodes: self.n_nodes,
            children: self.n_children,
            names: self.n_names,
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// STAGE THREE - `FlatInterface`
//
// Post-intern-ing, is built to the exact size necessary to contain all
// de-duplicated items.
////////////////////////////////////////////////////////////////////////////////

/// A flattened [`InterfaceInfo`]
///
/// The const parameters are the number of methods, nodes, children, and name
/// bytes, calculated with [`FlatScratch::lengths()`]. Use [`flat_interface!`]
/// rather than building this directly.
///
/// [`flat_interface!`]: crate::flat_interface
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlatInterface<
    const METHODS: usize,
    const NODES: usize,
    const CHILDREN: usize,
    const NAMES: usize,
> {
    /// Always [`FLAT_VERSION`]. Decoders should check this first.
    pub version: u32,
    /// Name of the interface
    pub name: NameRef,
    /// See [`InterfaceInfo::max_request_size`]
    pub max_request_size: Option<u32>,
    /// See [`InterfaceInfo::max_response_size`]
    pub max_response_size: Option<u32>,
    pub methods: [FlatMethod; METHODS],
    /// Unique nodes, each appearing after all of its children
    pub nodes: [FlatNode; NODES],
    /// Node indices, referred to by [`Span`]s
    pub children: [u32; CHILDREN],
    pub names: [u8; NAMES],
}

impl<const METHODS: usize, const NODES: usize, const CHILDREN: usize, const NAMES: usize>
    FlatInterface<METHODS, NODES, CHILDREN, NAMES>
{
    /// Copies a flattened interface out of scratch space.
    ///
    /// Fails (at compile time, when used in a const context) if the const
    /// parameters of `Self` don't match [`FlatScratch::lengths()`].
    pub const fn new<const S_NODES: usize, const S_CHILDREN: usize, const S_NAMES: usize>(
        scratch: &FlatScratch<METHODS, S_NODES, S_CHILDREN, S_NAMES>,
    ) -> Self {
        assert!(scratch.n_nodes == NODES, "wrong node count");
        assert!(scratch.n_children == CHILDREN, "wrong children count");
        assert!(scratch.n_names == NAMES, "wrong name byte count");
        let mut out = Self {
            version: FLAT_VERSION,
            name: scratch.name,
            max_request_size: scratch.max_request_size,
            max_response_size: scratch.max_response_size,
            methods: scratch.methods,
            nodes: [FlatNode::Unit; NODES],
            children: [0; CHILDREN],
            names: [0; NAMES],
        };
        let mut i = 0;
        while i < NODES {
            out.nodes[i] = scratch.nodes[i];
            i += 1;
        }
        let mut i = 0;
        while i < CHILDREN {
            out.children[i] = scratch.children[i];
            i += 1;
        }
        let mut i = 0;
        while i < NAMES {
            out.names[i] = scratch.names[i];
            i += 1;
        }
        out
    }
}

////////////////////////////////////////////////////////////////////////////////
// Helper methods to paper over the fact that const trait impls aren't a thing
// yet unfortunately.
////////////////////////////////////////////////////////////////////////////////

/// const PartialEq for NameRef
const fn namerefs_eq(a: &NameRef, b: &NameRef) -> bool {
    a.len == b.len && a.offset == b.offset
}

/// const PartialEq for Span
const fn spans_eq(a: &Span, b: &Span) -> bool {
    a.len == b.len && a.start == b.start
}

/// const PartialEq for Option<u32>
const fn opt_eq(a: &Option<u32>, b: &Option<u32>) -> bool {
    match (a, b) {
        (None, None) => true,
        (None, Some(_)) => false,
        (Some(_), None) => false,
        (Some(a), Some(b)) => *a == *b,
    }
}

/// const PartialEq for FlatNode
const fn nodes_eq(a: &FlatNode, b: &FlatNode) -> bool {
    match (a, b) {
        (FlatNode::Bool, FlatNode::Bool) => true,
        (FlatNode::I8, FlatNode::I8) => true,
        (FlatNode::U8, FlatNode::U8) => true,
        (FlatNode::I16, FlatNode::I16) => true,
        (FlatNode::I32, FlatNode::I32) => true,
        (FlatNode::I64, FlatNode::I64) => true,
        (FlatNode::I128, FlatNode::I128) => true,
        (FlatNode::U16, FlatNode::U16) => true,
        (FlatNode::U32, FlatNode::U32) => true,
        (FlatNode::U64, FlatNode::U64) => true,
        (FlatNode::U128, FlatNode::U128) => true,
        (FlatNode::Usize, FlatNode::Usize) => true,
        (FlatNode::Isize, FlatNode::Isize) => true,
        (FlatNode::F32, FlatNode::F32) => true,
        (FlatNode::F64, FlatNode::F64) => true,
        (FlatNode::Char, FlatNode::Char) => true,
        (FlatNode::Unit, FlatNode::Unit) => true,
        (FlatNode::Schema, FlatNode::Schema) => true,
        (FlatNode::DataUnit, FlatNode::DataUnit) => true,
        (FlatNode::String { max_len: max_len_a }, FlatNode::String { max_len: max_len_b }) => {
            opt_eq(max_len_a, max_len_b)
        }
        (
            FlatNode::ByteArray { max_len: max_len_a },
            FlatNode::ByteArray { max_len: max_len_b },
        ) => opt_eq(max_len_a, max_len_b),
        (FlatNode::Option { inner: inner_a }, FlatNode::Option { inner: inner_b }) => {
            *inner_a == *inner_b
        }
        (
            FlatNode::Seq {
                element: element_a,
                max_len: max_len_a,
            },
            FlatNode::Seq {
                element: element_b,
                max_len: max_len_b,
            },
        ) => *element_a == *element_b && opt_eq(max_len_a, max_len_b),
        (FlatNode::Tuple { elems: elems_a }, FlatNode::Tuple { elems: elems_b }) => {
            spans_eq(elems_a, elems_b)
        }
        (
            FlatNode::Map {
                key: key_a,
                val: val_a,
                max_len: max_len_a,
            },
            FlatNode::Map {
                key: key_b,
                val: val_b,
                max_len: max_len_b,
            },
        ) => *key_a == *key_b && *val_a == *val_b && opt_eq(max_len_a, max_len_b),
        (
            FlatNode::Struct {
                name: name_a,
                data: data_a,
            },
            FlatNode::Struct {
                name: name_b,
                data: data_b,
            },
        ) => namerefs_eq(name_a, name_b) && *data_a == *data_b,
        (
            FlatNode::Enum {
                name: name_a,
                variants: variants_a,
            },
            FlatNode::Enum {
                name: name_b,
                variants: variants_b,
            },
        ) => namerefs_eq(name_a, name_b) && spans_eq(variants_a, variants_b),
        (
            FlatNode::Variant {
                name: name_a,
                data: data_a,
            },
            FlatNode::Variant {
                name: name_b,
                data: data_b,
            },
        ) => namerefs_eq(name_a, name_b) && *data_a == *data_b,
        (
            FlatNode::Field {
                name: name_a,
                ty: ty_a,
            },
            FlatNode::Field {
                name: name_b,
                ty: ty_b,
            },
        ) => namerefs_eq(name_a, name_b) && *ty_a == *ty_b,
        (FlatNode::DataNewtype { inner: inner_a }, FlatNode::DataNewtype { inner: inner_b }) => {
            *inner_a == *inner_b
        }
        (FlatNode::DataTuple { elems: elems_a }, FlatNode::DataTuple { elems: elems_b }) => {
            spans_eq(elems_a, elems_b)
        }
        (FlatNode::DataStruct { fields: fields_a }, FlatNode::DataStruct { fields: fields_b }) => {
            spans_eq(fields_a, fields_b)
        }
        _ => false,
    }
}

/// Const version of `<usize as TryInto>::try_into().unwrap()`.
const fn to_u32(n: usize) -> u32 {
    assert!(n <= u32::MAX as usize, "value too large for a flat schema");
    n as u32
}

/// Const version of `usize.map(|n| n.try_into().unwrap())`
const fn to_u32_opt(n: Option<usize>) -> Option<u32> {
    match n {
        Some(n) => Some(to_u32(n)),
        None => None,
    }
}

#[cfg(feature = "std")]
pub use decode::*;

#[cfg(feature = "std")]
mod decode {
    //! Converting flattened interfaces back into owned schemas

    use postcard_schema_ng::key::Key;
    // Re-exported, as these are part of the decoded output
    pub use postcard_schema_ng::schema::owned::{
        OwnedData, OwnedDataModelType, OwnedNamedField, OwnedVariant,
    };

    use super::*;

    /// Errors when decoding a flattened interface
    #[derive(Debug, Clone, PartialEq, Eq)]
    #[non_exhaustive]
    pub enum DecodeError {
        /// The [`FLAT_VERSION`] is not supported.
        UnsupportedVersion(u32),
        /// A method referred to a node past the end of the nodes.
        MissingNode(usize),
        /// A node referred to a child that wasn't before it in the nodes.
        BadReference { parent: usize, child: usize },
        /// A span was out of bounds.
        BadSpan(Span),
        /// A node was found where a different kind of node was expected.
        UnexpectedNode { idx: usize, node: FlatNode },
        /// A name was out of bounds, or not UTF-8.
        BadName(NameRef),
    }

    impl core::fmt::Display for DecodeError {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            match self {
                Self::UnsupportedVersion(v) => {
                    write!(f, "unsupported flat schema version {v}")
                }
                Self::MissingNode(idx) => write!(f, "missing node {idx}"),
                Self::BadReference { parent, child } => {
                    write!(f, "node {parent} refers to node {child}")
                }
                Self::BadSpan(s) => write!(f, "bad span {s:?}"),
                Self::UnexpectedNode { idx, node } => {
                    write!(f, "unexpected node {node:?} at {idx}")
                }
                Self::BadName(n) => write!(f, "bad name {n:?}"),
            }
        }
    }

    impl std::error::Error for DecodeError {}

    /// An owned version of [`MethodInfo`]
    #[derive(Debug, Clone, PartialEq)]
    pub struct OwnedMethodInfo {
        pub name: String,
        pub key: Key,
        pub req_schema: OwnedDataModelType,
        pub resp_schema: OwnedDataModelType,
    }

    /// An owned version of [`InterfaceInfo`], plus the interface name
    #[derive(Debug, Clone, PartialEq)]
    pub struct OwnedInterfaceInfo {
        pub name: String,
        pub max_request_size: Option<usize>,
        pub max_response_size: Option<usize>,
        pub methods: Vec<OwnedMethodInfo>,
    }

    /// A borrowed view of a [`FlatInterface`] of any size
    ///
    /// This is useful for decoding a flattened interface that was read out of
    /// a binary, where the const parameters are not known statically.
    #[derive(Debug, Clone, Copy)]
    pub struct FlatInterfaceRef<'a> {
        pub version: u32,
        pub name: NameRef,
        pub max_request_size: Option<u32>,
        pub max_response_size: Option<u32>,
        pub methods: &'a [FlatMethod],
        pub nodes: &'a [FlatNode],
        pub children: &'a [u32],
        pub names: &'a [u8],
    }

    impl<const M: usize, const N: usize, const C: usize, const S: usize> FlatInterface<M, N, C, S> {
        /// A borrowed view of this interface
        pub fn as_ref(&self) -> FlatInterfaceRef<'_> {
            FlatInterfaceRef {
                version: self.version,
                name: self.name,
                max_request_size: self.max_request_size,
                max_response_size: self.max_response_size,
                methods: &self.methods,
                nodes: &self.nodes,
                children: &self.children,
                names: &self.names,
            }
        }
    }

    impl FlatInterfaceRef<'_> {
        /// Converts back into an owned interface description.
        pub fn decode(&self) -> Result<OwnedInterfaceInfo, DecodeError> {
            if self.version != FLAT_VERSION {
                return Err(DecodeError::UnsupportedVersion(self.version));
            }
            let methods = self
                .methods
                .iter()
                .map(|m| {
                    Ok(OwnedMethodInfo {
                        name: self.name(m.name)?,
                        key: Key::from_bytes(m.key),
                        req_schema: self.ty(self.root(m.rqst)?)?,
                        resp_schema: self.ty(self.root(m.resp)?)?,
                    })
                })
                .collect::<Result<_, DecodeError>>()?;
            Ok(OwnedInterfaceInfo {
                name: self.name(self.name)?,
                max_request_size: self.max_request_size.map(|n| n as usize),
                max_response_size: self.max_response_size.map(|n| n as usize),
                methods,
            })
        }

        fn name(&self, n: NameRef) -> Result<String, DecodeError> {
            let start = n.offset as usize;
            let bytes = start
                .checked_add(n.len as usize)
                .and_then(|end| self.names.get(start..end))
                .ok_or(DecodeError::BadName(n))?;
            let s = core::str::from_utf8(bytes).map_err(|_| DecodeError::BadName(n))?;
            Ok(s.to_string())
        }

        /// Checks a method's reference to a node.
        fn root(&self, idx: u32) -> Result<usize, DecodeError> {
            let idx = idx as usize;
            if idx < self.nodes.len() {
                Ok(idx)
            } else {
                Err(DecodeError::MissingNode(idx))
            }
        }

        /// Checks a node's reference to a child, which must be before it.
        fn child(&self, parent: usize, child: u32) -> Result<usize, DecodeError> {
            let child = child as usize;
            if child < parent {
                Ok(child)
            } else {
                Err(DecodeError::BadReference { parent, child })
            }
        }

        /// Checks a node's span of children.
        fn span(&self, parent: usize, span: Span) -> Result<Vec<usize>, DecodeError> {
            let start = span.start as usize;
            start
                .checked_add(span.len as usize)
                .and_then(|end| self.children.get(start..end))
                .ok_or(DecodeError::BadSpan(span))?
                .iter()
                .map(|&c| self.child(parent, c))
                .collect()
        }

        /// Decodes the type at node `idx`, which must be in bounds.
        fn ty(&self, idx: usize) -> Result<OwnedDataModelType, DecodeError> {
            use OwnedDataModelType as T;
            let max = |n: Option<u32>| n.map(|n| n as usize);
            Ok(match self.nodes[idx] {
                FlatNode::Bool => T::Bool,
                FlatNode::I8 => T::I8,
                FlatNode::U8 => T::U8,
                FlatNode::I16 => T::I16,
                FlatNode::I32 => T::I32,
                FlatNode::I64 => T::I64,
                FlatNode::I128 => T::I128,
                FlatNode::U16 => T::U16,
                FlatNode::U32 => T::U32,
                FlatNode::U64 => T::U64,
                FlatNode::U128 => T::U128,
                FlatNode::Usize => T::Usize,
                FlatNode::Isize => T::Isize,
                FlatNode::F32 => T::F32,
                FlatNode::F64 => T::F64,
                FlatNode::Char => T::Char,
                FlatNode::String { max_len } => T::String {
                    max_len: max(max_len),
                },
                FlatNode::ByteArray { max_len } => T::ByteArray {
                    max_len: max(max_len),
                },
                FlatNode::Option { inner } => {
                    T::Option(Box::new(self.ty(self.child(idx, inner)?)?))
                }
                FlatNode::Unit => T::Unit,
                FlatNode::Seq { element, max_len } => T::Seq {
                    element: Box::new(self.ty(self.child(idx, element)?)?),
                    max_len: max(max_len),
                },
                FlatNode::Tuple { elems } => T::Tuple(self.tys(idx, elems)?),
                FlatNode::Map { key, val, max_len } => T::Map {
                    key: Box::new(self.ty(self.child(idx, key)?)?),
                    val: Box::new(self.ty(self.child(idx, val)?)?),
                    max_len: max(max_len),
                },
                FlatNode::Struct { name, data } => T::Struct {
                    name: self.name(name)?.into(),
                    data: self.data(self.child(idx, data)?)?,
                },
                FlatNode::Enum { name, variants } => T::Enum {
                    name: self.name(name)?.into(),
                    variants: self
                        .span(idx, variants)?
                        .into_iter()
                        .map(|v| self.variant(v))
                        .collect::<Result<_, _>>()?,
                },
                FlatNode::Schema => T::Schema,
                node @ (FlatNode::Variant { .. }
                | FlatNode::Field { .. }
                | FlatNode::DataUnit
                | FlatNode::DataNewtype { .. }
                | FlatNode::DataTuple { .. }
                | FlatNode::DataStruct { .. }) => {
                    return Err(DecodeError::UnexpectedNode { idx, node });
                }
            })
        }

        fn tys(&self, parent: usize, span: Span) -> Result<Box<[OwnedDataModelType]>, DecodeError> {
            self.span(parent, span)?
                .into_iter()
                .map(|c| self.ty(c))
                .collect()
        }

        fn variant(&self, idx: usize) -> Result<OwnedVariant, DecodeError> {
            match self.nodes[idx] {
                FlatNode::Variant { name, data } => Ok(OwnedVariant {
                    name: self.name(name)?.into(),
                    data: self.data(self.child(idx, data)?)?,
                }),
                node => Err(DecodeError::UnexpectedNode { idx, node }),
            }
        }

        fn data(&self, idx: usize) -> Result<OwnedData, DecodeError> {
            Ok(match self.nodes[idx] {
                FlatNode::DataUnit => OwnedData::Unit,
                FlatNode::DataNewtype { inner } => {
                    OwnedData::Newtype(Box::new(self.ty(self.child(idx, inner)?)?))
                }
                FlatNode::DataTuple { elems } => OwnedData::Tuple(self.tys(idx, elems)?),
                FlatNode::DataStruct { fields } => OwnedData::Struct(
                    self.span(idx, fields)?
                        .into_iter()
                        .map(|f| self.field(f))
                        .collect::<Result<_, _>>()?,
                ),
                node => return Err(DecodeError::UnexpectedNode { idx, node }),
            })
        }

        fn field(&self, idx: usize) -> Result<OwnedNamedField, DecodeError> {
            match self.nodes[idx] {
                FlatNode::Field { name, ty } => Ok(OwnedNamedField {
                    name: self.name(name)?.into(),
                    ty: self.ty(self.child(idx, ty)?)?,
                }),
                node => Err(DecodeError::UnexpectedNode { idx, node }),
            }
        }
    }
}

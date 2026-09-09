//! Interface and Interface Methods

use postcard_schema_ng::{Schema, key::Key, schema::DataModelType};
use serde::{Deserialize, Serialize};

/// A trait containing the metadata of a given method of an method
pub trait Method {
    type Request<'rqst>: Schema + Serialize + Deserialize<'rqst>;
    type Response<'resp>: Schema + Serialize + Deserialize<'resp>;

    const NAME: &'static str;

    const KEY: Key =
        Key::for_2ty_path::<Self::Request<'static>, Self::Response<'static>>(Self::NAME);

    const INFO: MethodInfo = MethodInfo {
        name: Self::NAME,
        key: Self::KEY,
        req_schema: <Self::Request<'static> as Schema>::SCHEMA,
        resp_schema: <Self::Response<'static> as Schema>::SCHEMA,
    };
}

/// A struct containing metadata for a given method of an interface
#[derive(Debug, Clone, Copy)]
pub struct MethodInfo {
    pub name: &'static str,
    pub key: Key,
    pub req_schema: &'static DataModelType,
    pub resp_schema: &'static DataModelType,
}

/// A struct containing metadata for a given interface
#[derive(Debug, Clone, Copy)]
pub struct InterfaceInfo {
    pub max_request_size: Option<usize>,
    pub max_response_size: Option<usize>,
    pub methods: &'static [MethodInfo],
}

/// Calculated the largest request body payload in bytes.
///
/// Returns `None` if one or more request type is unbounded.
pub const fn rqst_body_max_buf_required(infos: &[MethodInfo]) -> Option<usize> {
    // Ensure that buffers have AT LEAST enough for the WireError type
    let mut max = crate::wire::WireError::SCHEMA.max_size().unwrap();
    let mut idx = 0;
    while idx < infos.len() {
        let Some(m) = infos[idx].req_schema.max_size() else {
            return None;
        };
        if m > max {
            max = m;
        }
        idx += 1;
    }
    Some(max)
}

/// Calculated the largest response body payload in bytes.
///
/// Returns `None` if one or more response type is unbounded.
pub const fn resp_body_max_buf_required(infos: &[MethodInfo]) -> Option<usize> {
    // Ensure that buffers have AT LEAST enough for the WireError type
    let mut max = crate::wire::WireError::SCHEMA.max_size().unwrap();
    let mut idx = 0;
    while idx < infos.len() {
        let Some(m) = infos[idx].resp_schema.max_size() else {
            return None;
        };
        if m > max {
            max = m;
        }
        idx += 1;
    }
    Some(max)
}

/// Calculates the total number of `MethodInfo`s in an array of array of
/// `MethodInfo`s.
pub const fn total_len(sets: &[&[MethodInfo]]) -> usize {
    let mut i = 0;
    let mut ct = 0;
    while i < sets.len() {
        ct += sets[i].len();
        i += 1;
    }
    ct
}

/// Flattens an array of array of `MethodInfo`s into a flat array of `MethodInfo`s.
///
/// N should be the total length of `sets`, calculated by [`total_len()`].
pub const fn flatten<const N: usize>(sets: &[&[MethodInfo]]) -> [MethodInfo; N] {
    pub const ONE: MethodInfo = MethodInfo {
        name: "",
        key: Key::from_bytes([0; 8]),
        req_schema: &DataModelType::Unit,
        resp_schema: &DataModelType::Unit,
    };

    let mut out = [ONE; N];
    let mut i = 0;
    let mut n = 0;
    while i < sets.len() {
        let mut k = 0;
        while k < sets[i].len() {
            out[n] = sets[i][k];
            k += 1;
            n += 1;
        }
        i += 1;
    }
    assert!(n == N);
    out
}

/// Create an array of [`Key`]s from an array of [`MethodInfo`]s.
///
/// N must equal `infos.len()`.
pub const fn extract_keys<const N: usize>(infos: &[MethodInfo]) -> [Key; N] {
    assert!(N == infos.len());
    let mut buf = [Key::from_bytes([0u8; 8]); N];
    let mut idx = 0;
    while idx < N {
        buf[idx] = infos[idx].key;
        idx += 1;
    }
    buf
}

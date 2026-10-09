//! Wire types

use postcard_schema_ng::{Schema, key::Key};
use serde::{Deserialize, Serialize};

use crate::io::server::ServerError;

/// Current header (wire format) version
///
/// * 0: header serialized with serde/postcard (varint `seqno`, key as an
///   8-tuple), 11–13 bytes
/// * 1: fixed [`HEADER_LEN`]-byte layout, see [`Header`]
pub const WIRE_VERSION: u8 = 1;

/// Length of an encoded [`Header`]
pub const HEADER_LEN: usize = 12;

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone, Copy, Schema)]
pub enum Operation {
    Request,
    Response,
    ErrorResponse,
}

impl Operation {
    const fn to_u8(self) -> u8 {
        match self {
            Operation::Request => 0,
            Operation::Response => 1,
            Operation::ErrorResponse => 2,
        }
    }

    const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Operation::Request),
            1 => Some(Operation::Response),
            2 => Some(Operation::ErrorResponse),
            _ => None,
        }
    }
}

/// Frame header, present on every request and response
///
/// The header is not encoded with serde: it has a fixed [`HEADER_LEN`]-byte
/// layout, which is much smaller to encode and decode on small targets than
/// a derived (and often inlined) postcard implementation.
///
/// | offset | size | field             |
/// | :----- | :--- | :----             |
/// | 0      | 1    | `version`         |
/// | 1      | 1    | `op`              |
/// | 2      | 2    | `seqno` u16, LE   |
/// | 4      | 8    | `key` u64, LE     |
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Header {
    pub version: u8,
    pub op: Operation,
    pub seqno: u16,
    pub key: Key,
}

impl Header {
    /// Writes the header to the start of `buf`, returning the remaining bytes
    /// or `None` if `buf` is too short.
    pub fn write_to<'a>(&self, buf: &'a mut [u8]) -> Option<&'a mut [u8]> {
        let (out, rest) = buf.split_first_chunk_mut::<HEADER_LEN>()?;
        out[0] = self.version;
        out[1] = self.op.to_u8();
        out[2..4].copy_from_slice(&self.seqno.to_le_bytes());
        out[4..12].copy_from_slice(&self.key.to_bytes());
        Some(rest)
    }

    /// Reads a header from the start of `buf`, returning it and the remaining
    /// (body) bytes, or `None` if `buf` is too short or the operation is
    /// unknown.
    pub fn read_from(buf: &[u8]) -> Option<(Self, &[u8])> {
        let (h, rest) = buf.split_first_chunk::<HEADER_LEN>()?;
        let mut key = [0u8; 8];
        key.copy_from_slice(&h[4..12]);
        let hedr = Self {
            version: h[0],
            op: Operation::from_u8(h[1])?,
            seqno: u16::from_le_bytes([h[2], h[3]]),
            key: Key::from_bytes(key),
        };
        Some((hedr, rest))
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Schema)]
pub struct WireError(pub u8);

impl WireError {
    // Server errors
    pub const SERVER_BAD_HEADER: Self = Self(10u8);
    pub const SERVER_UNEXPECTED_METHOD: Self = Self(11u8);
    pub const SERVER_VERSION_MISMATCH: Self = Self(12u8);
    pub const SERVER_UNKNOWN_METHOD: Self = Self(13u8);
    pub const SERVER_BAD_BODY: Self = Self(14u8);
    pub const SERVER_RESPOND_FAILED: Self = Self(15u8);

    pub const KEY: Key = Key::for_path::<Self>("error");
}

impl From<&ServerError> for WireError {
    fn from(value: &ServerError) -> Self {
        match value {
            ServerError::RequestHeaderDeserialize(_) => Self::SERVER_BAD_HEADER,
            ServerError::RequestWrongOperation => Self::SERVER_UNEXPECTED_METHOD,
            ServerError::RequestVersionMismatch => Self::SERVER_VERSION_MISMATCH,
            ServerError::UnknownMethod => Self::SERVER_UNKNOWN_METHOD,
            ServerError::RequestBodyDeserialize(_) => Self::SERVER_BAD_BODY,
            ServerError::ResponseSerialize(_) => Self::SERVER_RESPOND_FAILED,
        }
    }
}

impl From<ServerError> for WireError {
    fn from(value: ServerError) -> Self {
        Self::from(&value)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn header_layout() {
        let hedr = Header {
            version: WIRE_VERSION,
            op: Operation::Response,
            seqno: 0x1234,
            key: Key::from_bytes([1, 2, 3, 4, 5, 6, 7, 8]),
        };
        let mut buf = [0xFFu8; 16];
        assert_eq!(hedr.write_to(&mut buf), Some([0xFF; 4].as_mut_slice()));
        assert_eq!(
            buf[..HEADER_LEN],
            [WIRE_VERSION, 1, 0x34, 0x12, 1, 2, 3, 4, 5, 6, 7, 8]
        );

        buf[HEADER_LEN] = 0xAA;
        let (back, rest) = Header::read_from(&buf[..HEADER_LEN + 1]).unwrap();
        assert_eq!(back, hedr);
        assert_eq!(rest, &[0xAA]);
    }

    #[test]
    fn header_errors() {
        let hedr = Header {
            version: WIRE_VERSION,
            op: Operation::Request,
            seqno: 0,
            key: WireError::KEY,
        };
        // Too short to write or read
        assert_eq!(hedr.write_to(&mut [0u8; HEADER_LEN - 1]), None);
        assert!(Header::read_from(&[0u8; HEADER_LEN - 1]).is_none());
        // Unknown operation
        let mut buf = [0u8; HEADER_LEN];
        hedr.write_to(&mut buf).unwrap();
        buf[1] = 3;
        assert!(Header::read_from(&buf).is_none());
    }
}

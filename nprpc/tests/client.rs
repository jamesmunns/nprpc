//! Tests for how the client handles the frames it receives

use postcard_schema_ng::key::Key;

use nprpc::interface;
use nprpc::{
    ClientError, ClientIoError, autobuffer,
    interface::Method,
    io::client::{Backend, Io},
    wire::{HEADER_LEN, Header, HeaderError, Operation, WIRE_VERSION, WireError},
};
use serde::Serialize;

interface! {
    mod example {
        fn double(u32) -> u32;
    }
}

use example::Client as _;

const DOUBLE: Key = <example::methods::double as Method>::KEY;

autobuffer!(Bufs, example);

/// Replies to every request with the same canned frame
struct FakeIo {
    reply: Vec<u8>,
}

impl Io for FakeIo {
    type Error = ();

    fn send_then_receive_raw_frames<'a>(
        &mut self,
        _outgoing: &[u8],
        incoming: &'a mut [u8],
    ) -> Result<&'a [u8], ()> {
        let raw = &mut incoming[..self.reply.len()];
        raw.copy_from_slice(&self.reply);
        Ok(raw)
    }
}

struct TestBackend {
    bufs: Bufs,
    io: FakeIo,
}

impl Backend for TestBackend {
    type Storage = Bufs;
    type Io = FakeIo;

    /// Every request uses sequence number 42
    fn next_sequence_number(&mut self) -> u16 {
        42
    }

    fn parts(&mut self) -> (&mut Bufs, &mut FakeIo) {
        (&mut self.bufs, &mut self.io)
    }
}

fn frame<T: Serialize>(op: Operation, seqno: u16, key: Key, body: &T) -> Vec<u8> {
    let hedr = Header {
        version: WIRE_VERSION,
        op,
        seqno,
        key,
    };
    let mut buf = [0u8; 64];
    let mut out = Vec::new();
    let _remain = hedr.write_to(&mut buf).unwrap();
    out.extend_from_slice(&buf[..HEADER_LEN]);
    out.extend_from_slice(postcard::to_slice(body, &mut buf).unwrap());
    out
}

/// Call `double(21)`, receiving the given reply frame
fn call(reply: Vec<u8>) -> Result<u32, ClientIoError<()>> {
    let mut backend = TestBackend {
        bufs: Bufs::new(),
        io: FakeIo { reply },
    };
    backend.double(&21).map(|resp| resp.body)
}

#[test]
fn response() {
    let reply = frame(Operation::Response, 42, DOUBLE, &42u32);
    assert_eq!(call(reply), Ok(42));
}

#[test]
fn reject_trailing_bytes() {
    let mut reply = frame(Operation::Response, 42, DOUBLE, &42u32);
    reply.push(0);
    assert_eq!(
        call(reply),
        Err(ClientIoError::Client(
            ClientError::ResponseBodyTrailingBytes
        ))
    );

    let mut reply = frame(
        Operation::ErrorResponse,
        42,
        WireError::KEY,
        &WireError::SERVER_UNKNOWN_METHOD,
    );
    reply.push(0);
    assert_eq!(
        call(reply),
        Err(ClientIoError::Client(
            ClientError::ResponseBodyTrailingBytes
        ))
    );
}

#[test]
fn error_response() {
    let reply = frame(
        Operation::ErrorResponse,
        42,
        WireError::KEY,
        &WireError::SERVER_UNKNOWN_METHOD,
    );
    assert_eq!(
        call(reply),
        Err(ClientIoError::Client(ClientError::Server(
            WireError::SERVER_UNKNOWN_METHOD
        )))
    );
}

#[test]
fn error_response_without_seqno() {
    // The server couldn't decode our header, so doesn't know our seqno
    let reply = frame(
        Operation::ErrorResponse,
        0,
        WireError::KEY,
        &WireError::SERVER_BAD_HEADER,
    );
    assert_eq!(
        call(reply),
        Err(ClientIoError::Client(ClientError::Server(
            WireError::SERVER_BAD_HEADER
        )))
    );
}

#[test]
fn error_response_wrong_seqno() {
    let reply = frame(
        Operation::ErrorResponse,
        7,
        WireError::KEY,
        &WireError::SERVER_UNKNOWN_METHOD,
    );
    assert_eq!(
        call(reply),
        Err(ClientIoError::Client(ClientError::ResponseBadSeqno))
    );
}

#[test]
fn error_response_wrong_key() {
    let reply = frame(
        Operation::ErrorResponse,
        42,
        DOUBLE,
        &WireError::SERVER_UNKNOWN_METHOD,
    );
    assert_eq!(
        call(reply),
        Err(ClientIoError::Client(ClientError::ResponseKeyMismatch))
    );
}

#[test]
fn request_operation() {
    let reply = frame(Operation::Request, 42, DOUBLE, &42u32);
    assert_eq!(
        call(reply),
        Err(ClientIoError::Client(ClientError::ResponseBadOperation))
    );
}

#[test]
fn bad_header() {
    assert_eq!(
        call(Vec::from([WIRE_VERSION, 1])),
        Err(ClientIoError::Client(
            ClientError::ResponseHeaderDeserialize(HeaderError::BufferTooShort)
        ))
    );
}

#[test]
fn version_mismatch() {
    let mut reply = frame(Operation::Response, 42, DOUBLE, &42u32);
    reply[0] = WIRE_VERSION + 1;
    assert_eq!(
        call(reply),
        Err(ClientIoError::Client(ClientError::ResponseVersionMismatch))
    );
}

#[test]
fn raw_response() {
    // A raw call sees the same frame as a typed call, without (de)serializing
    let reply = frame(Operation::Response, 42, DOUBLE, &42u32);
    let mut backend = TestBackend {
        bufs: Bufs::new(),
        io: FakeIo { reply },
    };
    let resp = backend.send_then_receive_raw(DOUBLE, &[21]).unwrap();
    assert_eq!(resp.hedr.key, DOUBLE);
    assert_eq!(resp.hedr.seqno, 42);
    // Small `u32`s are a single varint byte
    assert_eq!(resp.body, &[42]);
}

#[test]
fn raw_checks_header() {
    // The response header is checked against the raw request's key
    let reply = frame(Operation::Response, 42, WireError::KEY, &42u32);
    let mut backend = TestBackend {
        bufs: Bufs::new(),
        io: FakeIo { reply },
    };
    assert_eq!(
        backend.send_then_receive_raw(DOUBLE, &[21]),
        Err(ClientIoError::Client(ClientError::ResponseKeyMismatch))
    );
}

#[test]
fn raw_request_too_large() {
    let mut backend = TestBackend {
        bufs: Bufs::new(),
        io: FakeIo { reply: vec![] },
    };
    assert_eq!(
        backend.send_then_receive_raw(DOUBLE, &[0; 64]),
        Err(ClientIoError::Client(ClientError::RequestSerialize(
            postcard::Error::SerializeBufferFull
        )))
    );
}

//! Tests for what `serve_one` reports, and what it sends to the client

use postcard_schema_ng::{key::Key, max_len::MaxLenStr};

use nprpc::interface;
use nprpc::{
    Request, Served, ServerError, ServerIoError, autobuffer,
    interface::Method,
    io::server::{Backend, Io, RawIoFrame},
    wire::{HEADER_LEN, Header, Operation, WIRE_VERSION, WireError},
};
use serde::Serialize;

interface! {
    mod example {
        fn double(u32) -> u32;
        fn shout(MaxLenStr<'rqst, 4>) -> ();
    }
}

use example::Server as _;

const DOUBLE: Key = <example::methods::double as Method>::KEY;
const SHOUT: Key = <example::methods::shout as Method>::KEY;

autobuffer!(Bufs, example);

struct ServerImpl;

impl example::Server for ServerImpl {
    fn double(&mut self, rqst: Request<u32>) -> u32 {
        *rqst.body * 2
    }

    fn shout(&mut self, _rqst: Request<MaxLenStr<'_, 4>>) {}
}

/// Holds (at most) one incoming frame, and records all outgoing frames
#[derive(Default)]
struct FakeIo {
    incoming: Option<Vec<u8>>,
    outgoing: Vec<Vec<u8>>,
    fail_send: bool,
}

impl Io for FakeIo {
    type Meta = ();
    type Error = &'static str;

    fn recv_one_frame_raw<'data>(
        &mut self,
        incoming: &'data mut [u8],
    ) -> Result<Option<RawIoFrame<'data, ()>>, ServerIoError<Self::Error>> {
        let Some(frame) = self.incoming.take() else {
            return Ok(None);
        };
        let raw = &mut incoming[..frame.len()];
        raw.copy_from_slice(&frame);
        Ok(Some(RawIoFrame { meta: (), raw }))
    }

    fn send_one_frame_raw(
        &mut self,
        outgoing: RawIoFrame<'_, ()>,
    ) -> Result<(), ServerIoError<Self::Error>> {
        if self.fail_send {
            return Err(ServerIoError::Io("send failed"));
        }
        self.outgoing.push(outgoing.raw.to_vec());
        Ok(())
    }
}

struct TestBackend {
    bufs: Bufs,
    io: FakeIo,
}

impl Backend for TestBackend {
    type Storage = Bufs;
    type Io = FakeIo;

    fn parts(&mut self) -> (&mut Bufs, &mut FakeIo) {
        (&mut self.bufs, &mut self.io)
    }
}

type ServeResult = Result<Served, ServerIoError<&'static str>>;

fn frame<T: Serialize>(op: Operation, version: u8, key: Key, body: &T) -> Vec<u8> {
    let hedr = Header {
        version,
        op,
        seqno: 42,
        key,
    };
    let mut buf = [0u8; 64];
    let mut out = Vec::new();
    let remain = hedr.write_to(&mut buf).unwrap();
    out.extend_from_slice(&buf[..HEADER_LEN]);
    out.extend_from_slice(postcard::to_slice(body, &mut buf).unwrap());
    out
}

/// Serve the given incoming frame (if any), returning the result and all
/// of the frames that were sent to the client.
fn serve(incoming: Option<Vec<u8>>) -> (ServeResult, Vec<Vec<u8>>) {
    let mut backend = TestBackend {
        bufs: Bufs::new(),
        io: FakeIo {
            incoming,
            ..FakeIo::default()
        },
    };
    let res = ServerImpl.serve_one(&mut backend);
    (res, backend.io.outgoing)
}

/// Assert that exactly one frame was sent, that it is an error response
/// with the given sequence number, and that it holds the given error.
#[track_caller]
fn assert_error_frame(sent: &[Vec<u8>], seqno: u16, expected: WireError) {
    let [sent] = sent else {
        panic!("expected exactly one frame, got {sent:?}");
    };
    let (hedr, body) = Header::read_from(sent).unwrap();
    assert_eq!(hedr.op, Operation::ErrorResponse);
    assert_eq!(hedr.seqno, seqno);
    assert_eq!(hedr.key, WireError::KEY);
    assert_eq!(postcard::from_bytes::<WireError>(body).unwrap(), expected);
}

#[test]
fn idle() {
    let (res, sent) = serve(None);
    assert_eq!(res, Ok(Served::Idle));
    assert!(sent.is_empty());
}

#[test]
fn responded() {
    let rqst = frame(Operation::Request, WIRE_VERSION, DOUBLE, &21u32);
    let (res, sent) = serve(Some(rqst));
    assert_eq!(res, Ok(Served::Responded));

    let [sent] = sent.as_slice() else {
        panic!("expected exactly one frame, got {sent:?}");
    };
    let (hedr, body) = Header::read_from(sent).unwrap();
    assert_eq!(hedr.op, Operation::Response);
    assert_eq!(hedr.seqno, 42);
    assert_eq!(hedr.key, DOUBLE);
    assert_eq!(postcard::from_bytes::<u32>(body).unwrap(), 42);
}

#[test]
fn bad_header() {
    // Too short to contain a header
    let (res, sent) = serve(Some(Vec::from([0u8, 0])));
    assert!(matches!(
        res,
        Ok(Served::Rejected(ServerError::RequestHeaderDeserialize(_)))
    ));
    // No header was decoded, so there is no sequence number to copy
    assert_error_frame(&sent, 0, WireError::SERVER_BAD_HEADER);
}

#[test]
fn unknown_method() {
    let rqst = frame(Operation::Request, WIRE_VERSION, WireError::KEY, &21u32);
    let (res, sent) = serve(Some(rqst));
    assert_eq!(res, Ok(Served::Rejected(ServerError::UnknownMethod)));
    assert_error_frame(&sent, 42, WireError::SERVER_UNKNOWN_METHOD);
}

#[test]
fn bad_body() {
    // A valid header for `shout`, but the body claims to be a string that
    // is longer than the remaining data.
    let mut rqst = frame(Operation::Request, WIRE_VERSION, SHOUT, &());
    rqst.push(4);
    let (res, sent) = serve(Some(rqst));
    assert!(matches!(
        res,
        Ok(Served::Rejected(ServerError::RequestBodyDeserialize(_)))
    ));
    assert_error_frame(&sent, 42, WireError::SERVER_BAD_BODY);
}

#[test]
fn wrong_operation() {
    for op in [Operation::Response, Operation::ErrorResponse] {
        let rqst = frame(op, WIRE_VERSION, DOUBLE, &21u32);
        let (res, sent) = serve(Some(rqst));
        assert_eq!(
            res,
            Ok(Served::Rejected(ServerError::RequestWrongOperation))
        );
        assert_error_frame(&sent, 42, WireError::SERVER_UNEXPECTED_METHOD);
    }
}

#[test]
fn version_mismatch() {
    let rqst = frame(Operation::Request, WIRE_VERSION + 1, DOUBLE, &21u32);
    let (res, sent) = serve(Some(rqst));
    assert_eq!(
        res,
        Ok(Served::Rejected(ServerError::RequestVersionMismatch))
    );
    assert_error_frame(&sent, 42, WireError::SERVER_VERSION_MISMATCH);
}

#[test]
fn io_errors_are_errors() {
    let good = frame(Operation::Request, WIRE_VERSION, DOUBLE, &21u32);
    let bad = frame(Operation::Request, WIRE_VERSION, WireError::KEY, &21u32);
    for rqst in [good, bad] {
        let mut backend = TestBackend {
            bufs: Bufs::new(),
            io: FakeIo {
                incoming: Some(rqst),
                fail_send: true,
                ..FakeIo::default()
            },
        };
        let res = ServerImpl.serve_one(&mut backend);
        assert_eq!(res, Err(ServerIoError::Io("send failed")));
    }
}

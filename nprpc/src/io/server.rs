use core::fmt::Debug;
use postcard::{
    Serializer,
    ser_flavors::{Flavor as _, Slice as SerSlice},
};
use serde::Serialize;

use crate::{
    Request, RequestRaw,
    interface::Endpoint,
    io::{Storage, StorageView},
    wire::{Header, Method, WireError},
};

/// Raw frame received from or sent to the [`Io`] implementation.
///
/// This bundles any implementation-specific metadata, such as a peer address
/// for UDP comms, with the raw frame (which contains a serialized header and
/// body).
pub struct RawIoFrame<'data, T> {
    /// [`Io`] instance specific metadata, if any.
    pub meta: T,
    /// Raw serialized frame (header and body).
    pub raw: &'data [u8],
}

/// Server Errors
#[derive(Debug, PartialEq)]
pub enum ServerError {
    /// Received a frame, but failed to deserialize a [`Header`] from the frame.
    RequestHeaderDeserialize(postcard::Error),
    /// Received a request with an unexpected method field.
    RequestWrongMethod,
    /// Received a request with an unexpected version field.
    RequestVersionMismatch,
    /// Received a request for an [`Endpoint`] [`Key`] this server is not
    /// capable of handling
    UnknownEndpoint,
    /// Received a frame and deserialized the header, but failed to deserialize
    /// the request body.
    RequestBodyDeserialize(postcard::Error),
    /// Failed to serialize a response after processing a request.
    ResponseSerialize(postcard::Error),
}

/// Error with the server
///
/// Can either be an `Io` error with the underlying I/O (while exchanging
/// frames), or a `Server` error, if the frame contained invalid or
/// unexpected data.
#[derive(Debug, PartialEq)]
pub enum ServerIoError<E> {
    Server(ServerError),
    Io(E),
}

impl<E> From<ServerError> for ServerIoError<E> {
    fn from(value: ServerError) -> Self {
        Self::Server(value)
    }
}

/// The `Io` abstraction for a client
///
/// This interface is responsible for receiving a request frame from the
/// client, and sending a response frame.
pub trait Io {
    /// Metadata, like the peer address for UDP
    type Meta;

    /// The I/O specific error type
    type Error: Debug;

    /// Receive a single raw frame, containing a serialized header and body.
    fn recv_one_frame_raw<'data>(
        &mut self,
        incoming: &'data mut [u8],
    ) -> Result<Option<RawIoFrame<'data, Self::Meta>>, ServerIoError<Self::Error>>;

    /// Transmit a single raw frame, containing a serialized header and body.
    ///
    /// The frame is transmitted with a copy of the [`Io::Meta`] that was
    /// obtained from the previous call to [`Io::recv_one_frame_raw`],
    /// unmodified.
    fn send_one_frame_raw(
        &mut self,
        outgoing: RawIoFrame<'_, Self::Meta>,
    ) -> Result<(), ServerIoError<Self::Error>>;

    /// Transmit an error response.
    ///
    /// We may or may not have a header, depending on whether decoding of the
    /// incoming header was successful or not, and at what stage the error
    /// occurred.
    ///
    /// The error response will have the `ErrorResponse` method type and the
    /// `WireError::KEY`. If a header was decoded, the sequence number will be
    /// copied from that header, otherwise `0`.
    ///
    /// The body of the response will be of type `WireError`, and serialied
    /// into the `resp_buf`.
    fn send_one_error(
        &mut self,
        meta: Self::Meta,
        hdr: Option<Header>,
        err: WireError,
        resp_buf: &mut [u8],
    ) -> Result<(), ServerIoError<Self::Error>> {
        let hdr = Header {
            version: 0,
            method: Method::ErrorResponse,
            seqno: hdr.map(|h| h.seqno).unwrap_or(0),
            key: WireError::KEY,
        };

        let mut out = Serializer {
            output: SerSlice::new(resp_buf),
        };
        hdr.serialize(&mut out)
            .map_err(ServerError::ResponseSerialize)?;
        err.serialize(&mut out)
            .map_err(ServerError::ResponseSerialize)?;
        // TODO: CRC? Wrapping flavor?
        let used = out
            .output
            .finalize()
            .map_err(ServerError::ResponseSerialize)?;

        let frame = RawIoFrame { meta, raw: used };

        self.send_one_frame_raw(frame)
    }
}

// TODO: "Backend" isn't a very meaningful name. Come up with a name that better
// gets across that this contains room for ser/de as well as the I/O portion of
// the work.
pub trait Backend {
    type Storage: Storage;
    type Io: Io;
    fn parts(&mut self) -> (&mut Self::Storage, &mut Self::Io);
}

pub fn serve_one_with_dispatcher<B: Backend>(
    backend: &mut B,
    dispatcher: impl for<'a> FnOnce(RequestRaw<'_>, &'a mut [u8]) -> Result<&'a [u8], ServerError>,
) -> Result<(), ServerIoError<<B::Io as Io>::Error>> {
    let (sto, intfc) = backend.parts();
    let StorageView { rqst_buf, resp_buf } = sto.buffers();

    // If we got a fatal wire error, return it with ?
    // If we got no error but no packet, nothing to do
    let Some(frame) = intfc.recv_one_frame_raw(rqst_buf)? else {
        return Ok(());
    };

    let Ok((hdr, remain)) = postcard::take_from_bytes::<Header>(frame.raw) else {
        return intfc.send_one_error(frame.meta, None, WireError::SERVER_BAD_HEADER, resp_buf);
    };

    if hdr.method != Method::Request {
        return Err(ServerIoError::Server(ServerError::RequestWrongMethod));
    }
    if hdr.version != 0 {
        return Err(ServerIoError::Server(ServerError::RequestVersionMismatch));
    }

    let rqst_raw = RequestRaw {
        hdr: hdr.clone(),
        rqst: remain,
    };

    let res = dispatcher(rqst_raw, resp_buf);
    match res {
        Ok(outgoing) => intfc.send_one_frame_raw(RawIoFrame {
            meta: frame.meta,
            raw: outgoing,
        }),
        Err(e) => intfc.send_one_error(frame.meta, Some(hdr), e.into(), resp_buf),
    }
}

pub fn process_endpoint_request<'req, 'resp, 'out, E: Endpoint>(
    req_raw: RequestRaw<'req>,
    out: &'out mut [u8],
    func: impl FnOnce(Request<E::Request<'req>>) -> E::Response<'resp>,
) -> Result<&'out [u8], ServerError> {
    let RequestRaw { mut hdr, rqst } = req_raw;

    // Deserialize
    let body: E::Request<'_> =
        postcard::from_bytes(rqst).map_err(ServerError::RequestBodyDeserialize)?;

    // TODO: ensure all bytes consumed?

    // Process request
    // TODO: Pass hdr + req by reference? Probably no need to copy/move.
    let req = Request {
        hdr: hdr.clone(),
        req: body,
    };
    let resp = func(req);
    // Serialize response
    let mut out = Serializer {
        output: SerSlice::new(out),
    };
    hdr.method = Method::Response;
    hdr.serialize(&mut out)
        .map_err(ServerError::ResponseSerialize)?;
    resp.serialize(&mut out)
        .map_err(ServerError::ResponseSerialize)?;
    let used = out
        .output
        .finalize()
        .map_err(ServerError::ResponseSerialize)?;
    Ok(used)
}

//! Items relevant for server and server operations

use core::{fmt::Debug, ops::ControlFlow};

use crate::{
    RequestRaw,
    io::{Storage, StorageView},
    wire::{HEADER_LEN, Header, HeaderError, Operation, WIRE_VERSION, WireError},
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
#[non_exhaustive]
pub enum ServerError {
    /// Received a frame, but failed to deserialize a [`Header`] from the frame.
    ///
    /// A version mismatch is reported as [`ServerError::RequestVersionMismatch`]
    /// instead.
    RequestHeaderDeserialize(HeaderError),
    /// Received a request with an unexpected operation field.
    RequestWrongOperation,
    /// Received a request with an unexpected version field.
    RequestVersionMismatch,
    /// Received a request for a [`Method`]'s [`Key`] that this server is not
    /// capable of handling
    ///
    /// [`Key`]: postcard_schema_ng::key::Key
    /// [`Method`]: crate::interface::Method
    UnknownMethod,
    /// Received a frame and deserialized the header, but failed to deserialize
    /// the request body.
    RequestBodyDeserialize(postcard::Error),
    /// Deserialized the request body, but the frame contained extra bytes
    /// after it.
    RequestBodyTrailingBytes,
    /// Failed to serialize the header of a response, e.g. when the response
    /// buffer was too small.
    ResponseHeaderSerialize(HeaderError),
    /// Failed to serialize a response after processing a request.
    ResponseSerialize(postcard::Error),
}

/// Error with the server
#[derive(Debug, PartialEq)]
#[non_exhaustive]
pub enum ServerIoError<E> {
    /// The server failed to produce a frame to send, e.g. when the response
    /// buffer was too small.
    Server(ServerError),
    /// An error with the underlying `Io` implementation, e.g. when we failed
    /// to exchange frames.
    Io(E),
}

impl<E> From<ServerError> for ServerIoError<E> {
    fn from(value: ServerError) -> Self {
        Self::Server(value)
    }
}

/// The `Io` abstraction for a server
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
    /// The body of the response will be of type `WireError`, and serialized
    /// into the `resp_buf`.
    fn send_one_error(
        &mut self,
        meta: Self::Meta,
        hedr: Option<Header>,
        err: WireError,
        resp_buf: &mut [u8],
    ) -> Result<(), ServerIoError<Self::Error>> {
        let hedr = Header {
            version: WIRE_VERSION,
            op: Operation::ErrorResponse,
            seqno: hedr.map(|h| h.seqno).unwrap_or(0),
            key: WireError::KEY,
        };

        let remain = hedr
            .write_to(resp_buf)
            .map_err(ServerError::ResponseHeaderSerialize)?;
        let body_len = postcard::to_slice(&err, remain)
            .map_err(ServerError::ResponseSerialize)?
            .len();
        let used = &resp_buf[..HEADER_LEN + body_len];

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

/// The outcome of a `Server::try_dispatch_one` call.
///
/// You don't usually need to touch this, and should prefer using `dispatch_one`
/// instead, which returns a `Result`.
///
/// * `Break`: the request was for one of the server's methods. Contains the
///   serialized response frame, or the error that occurred while handling it.
/// * `Continue`: the request was not for any of the server's methods. Contains
///   the untouched output buffer, so the request can be offered elsewhere.
pub type Dispatched<'buf> = ControlFlow<Result<&'buf [u8], ServerError>, &'buf mut [u8]>;

/// What a call to `serve_one` did.
///
/// Returned by [`serve_one_with_dispatcher`], and by the `Server::serve_one`
/// methods generated by [`interface!`](crate::interface!) and
/// [`compose_interfaces!`](crate::compose_interfaces).
#[derive(Debug, PartialEq)]
#[must_use = "a `Rejected` request may indicate a misbehaving client"]
#[non_exhaustive]
pub enum Served {
    /// The [`Io`] had no request ready, nothing was sent.
    Idle,
    /// A request was dispatched and its response was sent. This is the typical
    /// expected outcome.
    Responded,
    /// A frame was received, but it could not be served. The client was sent
    /// a [`WireError`] corresponding to the contained [`ServerError`].
    Rejected(ServerError),
}

/// Serve a single response with the given server dispatcher function
///
/// On success, [`Served`] is returned, reporting whether nothing, an error,
/// or a normal response was sent.
///
/// On an exceptional error (such as a problem with the `Io` impl) occurred,
/// indicating we were unable to receive or send a response at all.
pub fn serve_one_with_dispatcher<B: Backend>(
    backend: &mut B,
    dispatcher: impl for<'a> FnOnce(RequestRaw<'_>, &'a mut [u8]) -> Result<&'a [u8], ServerError>,
) -> Result<Served, ServerIoError<<B::Io as Io>::Error>> {
    let (sto, intfc) = backend.parts();
    let StorageView { rqst_buf, resp_buf } = sto.buffers();

    // If we got a fatal wire error, return it with ?
    // If we got no error but no packet, nothing to do
    let Some(frame) = intfc.recv_one_frame_raw(rqst_buf)? else {
        return Ok(Served::Idle);
    };

    let (hedr, remain) = match Header::read_from(frame.raw) {
        Ok(h) => h,
        Err(e) => {
            let err = match e {
                HeaderError::UnsupportedVersion(_) => ServerError::RequestVersionMismatch,
                e => ServerError::RequestHeaderDeserialize(e),
            };
            intfc.send_one_error(frame.meta, None, (&err).into(), resp_buf)?;
            return Ok(Served::Rejected(err));
        }
    };

    let res = if hedr.op != Operation::Request {
        Err(ServerError::RequestWrongOperation)
    } else {
        let rqst_raw = RequestRaw {
            hedr: &hedr,
            body: remain,
        };
        dispatcher(rqst_raw, resp_buf)
    };

    match res {
        Ok(outgoing) => {
            intfc.send_one_frame_raw(RawIoFrame {
                meta: frame.meta,
                raw: outgoing,
            })?;
            Ok(Served::Responded)
        }
        Err(err) => {
            intfc.send_one_error(frame.meta, Some(hedr), (&err).into(), resp_buf)?;
            Ok(Served::Rejected(err))
        }
    }
}

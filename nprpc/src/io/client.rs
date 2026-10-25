use core::fmt::Debug;

use postcard_schema_ng::key::Key;

use crate::{
    Response,
    interface::Method,
    io::{Storage, StorageView},
    wire::{HEADER_LEN, Header, HeaderError, Operation, WIRE_VERSION, WireError},
};

/// Error with the client
///
/// Can either be an `Io` error with the underlying I/O (while exchanging
/// frames), or a `Client` error, if the frame contained invalid or
/// unexpected data.
#[derive(Debug, PartialEq)]
#[non_exhaustive]
pub enum ClientIoError<E> {
    /// Experienced an error while performing protocol operations.
    Client(ClientError),
    /// Experienced an error with the underlying I/O layer while exchanging
    /// frames.
    Io(E),
}

/// Client Errors
#[derive(Debug, PartialEq)]
#[non_exhaustive]
pub enum ClientError {
    /// Failed to serialize the header of the request, e.g. when the request
    /// buffer was too small.
    RequestHeaderSerialize(HeaderError),
    /// Failed to serialize the request.
    RequestSerialize(postcard::Error),
    /// Received a response, but unable to deserialize a header from the
    /// (alleged) response.
    ///
    /// A version mismatch is reported as [`ClientError::ResponseVersionMismatch`]
    /// instead.
    ResponseHeaderDeserialize(HeaderError),
    /// Received a reasonable `Header`, but unable to deserialize the body
    /// of the request.
    ResponseBodyDeserialize(postcard::Error),
    /// Deserialized the response body, but the frame contained extra bytes
    /// after it.
    ResponseBodyTrailingBytes,
    /// The client and server disagreed on the protocol version
    ResponseVersionMismatch,
    /// The response contained an unexpected Operation field.
    ResponseBadOperation,
    /// The response contained an unexpected sequence number.
    ResponseBadSeqno,
    /// The response contained an unexpected type tag [`Key`].
    ///
    /// [`Key`]: postcard_schema_ng::key::Key
    ResponseKeyMismatch,
    /// The server was unable to serve the request, and replied with an
    /// error instead.
    Server(WireError),
}

impl<E> From<ClientError> for ClientIoError<E> {
    fn from(value: ClientError) -> Self {
        Self::Client(value)
    }
}

/// The `Io` abstraction for a client
///
/// This interface is responsible for sending a request frame to the
/// destination, and receiving a response frame.
pub trait Io {
    type Error: Debug;

    /// Send a raw frame, then receive a raw frame.
    ///
    /// This uses the provided buffers: `outgoing` to be sent to the
    /// destination, and `incoming` for the received frame.
    ///
    /// `outgoing` contains a fully serialized header and body of the request.
    ///
    /// On success, the portion of `incoming` containing a valid frame is
    /// returned. This should contain both the header and body of the reply.
    fn send_then_receive_raw_frames<'a>(
        &mut self,
        outgoing: &[u8],
        incoming: &'a mut [u8],
    ) -> Result<&'a [u8], Self::Error>;
}

// TODO: "Backend" isn't a very meaningful name. Come up with a name that better
// gets across that this contains room for ser/de as well as the I/O portion of
// the work.
pub trait Backend {
    /// The [`Storage`] implementation used by this [`Backend`].
    type Storage: Storage;
    /// The [`Io`] implementation used by this [`Backend`].
    type Io: Io;

    /// Obtain the next sequence number to be used for outgoing requests.
    ///
    /// Typically a wrapping counter, though not required.
    fn next_sequence_number(&mut self) -> u16;

    /// Obtain the [`Storage`] and [`Io`] components of this backend.
    ///
    /// Provided as a single method to avoid borrowing issues of borrowing
    /// `self` twice.
    fn parts(&mut self) -> (&mut Self::Storage, &mut Self::Io);

    /// Send a request and then attempt to receive a response for the given
    /// [`Method`] type `M`, which bundles the request type as `M::Request`,
    /// the response type as `M::Response`, and the hash of the two schemas
    /// and path as the associated const `M::KEY`.
    ///
    /// This method prepares a header, then serializes an outgoing frame
    /// including the serialized header and body into the buffer provided by
    /// the [`Storage`] implementation.
    ///
    /// We then send that raw frame and attempt to receive a reply using the
    /// [`Io`] implementation.
    ///
    /// Finally, we attempt to deserialize that raw frame into a header and
    /// body, and return the result to the caller.
    fn send_then_receive_typed_frames<'rqst, 'resp, M: Method>(
        &'resp mut self,
        rqst: &M::Request<'rqst>,
    ) -> Result<Response<M::Response<'resp>>, ClientIoError<<Self::Io as Io>::Error>> {
        let RawResponse { hedr, body } = exchange(self, M::KEY, |buf| {
            postcard::to_slice(rqst, buf)
                .map(|used| used.len())
                .map_err(ClientError::RequestSerialize)
        })?;
        let body = take_body::<M::Response<'resp>>(body)?;
        Ok(Response { hedr, body })
    }

    /// Like [`Backend::send_then_receive_typed_frames`], but with an already
    /// serialized request body, and returning the response body without
    /// deserializing it.
    ///
    /// This is useful for clients that don't know the types of the interface
    /// statically, e.g. ones that transcode requests and responses using the
    /// interface's schemas. The caller is responsible for using the right
    /// `key` for the request body.
    ///
    /// The response header is checked the same way as for typed frames.
    fn send_then_receive_raw<'resp>(
        &'resp mut self,
        key: Key,
        rqst_body: &[u8],
    ) -> Result<RawResponse<'resp>, ClientIoError<<Self::Io as Io>::Error>> {
        exchange(self, key, |buf| {
            buf.get_mut(..rqst_body.len())
                .ok_or(ClientError::RequestSerialize(
                    postcard::Error::SerializeBufferFull,
                ))?
                .copy_from_slice(rqst_body);
            Ok(rqst_body.len())
        })
    }
}

/// A response with a body that has not been deserialized
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RawResponse<'resp> {
    pub hedr: Header,
    pub body: &'resp [u8],
}

/// Sends a request, and receives and checks the response header.
///
/// `write_body` writes the request body to the given buffer, returning the
/// number of bytes used.
fn exchange<'resp, B, F>(
    backend: &'resp mut B,
    key: Key,
    write_body: F,
) -> Result<RawResponse<'resp>, ClientIoError<<B::Io as Io>::Error>>
where
    B: Backend + ?Sized,
    F: FnOnce(&mut [u8]) -> Result<usize, ClientError>,
{
    let seqno = backend.next_sequence_number();
    let (storage, interface) = backend.parts();
    let StorageView { rqst_buf, resp_buf } = storage.buffers();

    // SERIALIZE OUTGOING...
    let hedr_rqst = Header {
        op: Operation::Request,
        version: WIRE_VERSION,
        seqno,
        key,
    };
    let remain = hedr_rqst
        .write_to(rqst_buf)
        .map_err(ClientError::RequestHeaderSerialize)?;
    let body_len = write_body(remain)?;
    let used = &rqst_buf[..HEADER_LEN + body_len];

    // Exchange...
    let recvd = interface
        .send_then_receive_raw_frames(used, resp_buf)
        .map_err(ClientIoError::Io)?;

    // DESERIALIZE INCOMING
    let (hedr_resp, body) = Header::read_from(recvd).map_err(|e| match e {
        HeaderError::UnsupportedVersion(_) => ClientError::ResponseVersionMismatch,
        e => ClientError::ResponseHeaderDeserialize(e),
    })?;

    // Check that response header matches all the qualities that we
    // expect...

    // The server replies with an error if it couldn't serve our request.
    // If it couldn't decode our header at all, it doesn't know our
    // sequence number, and uses `0` instead.
    if hedr_resp.op == Operation::ErrorResponse {
        if hedr_resp.seqno != hedr_rqst.seqno && hedr_resp.seqno != 0 {
            return Err(ClientError::ResponseBadSeqno.into());
        }
        if hedr_resp.key != WireError::KEY {
            return Err(ClientError::ResponseKeyMismatch.into());
        }
        let err = take_body::<WireError>(body)?;
        return Err(ClientError::Server(err).into());
    }
    if hedr_resp.op != Operation::Response {
        return Err(ClientError::ResponseBadOperation.into());
    }
    if hedr_resp.seqno != hedr_rqst.seqno {
        return Err(ClientError::ResponseBadSeqno.into());
    }
    if hedr_resp.key != hedr_rqst.key {
        return Err(ClientError::ResponseKeyMismatch.into());
    }

    Ok(RawResponse {
        hedr: hedr_resp,
        body,
    })
}

/// Deserializes a response body, which must use all of `body`
fn take_body<'de, T: serde::Deserialize<'de>>(body: &'de [u8]) -> Result<T, ClientError> {
    let (body, rest) =
        postcard::take_from_bytes::<T>(body).map_err(ClientError::ResponseBodyDeserialize)?;
    if !rest.is_empty() {
        return Err(ClientError::ResponseBodyTrailingBytes);
    }
    Ok(body)
}

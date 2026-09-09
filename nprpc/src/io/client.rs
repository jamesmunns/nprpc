use core::fmt::Debug;
use postcard::{
    Deserializer, Serializer,
    de_flavors::Slice as DeSlice,
    ser_flavors::{Flavor as _, Slice as SerSlice},
};
use serde::{Deserialize, Serialize};

use crate::{
    Response,
    interface::Endpoint,
    io::{Storage, StorageView},
    wire::{Header, Method},
};

/// Error with the client
///
/// Can either be an `Io` error with the underlying I/O (while exchanging
/// frames), or a `Client` error, if the frame contained invalid or
/// unexpected data.
#[derive(Debug, PartialEq)]
pub enum ClientIoError<E> {
    /// Experienced an error while performing protocol operations.
    Client(ClientError),
    /// Experienced an error with the underlying I/O layer while exchanging
    /// frames.
    Io(E),
}

/// Client Errors
#[derive(Debug, PartialEq)]
pub enum ClientError {
    /// Failed to serialize the request.
    RequestSerialize(postcard::Error),
    /// Received a response, but unable to deserialize a header from the
    /// (alleged) response.
    ResponseHeaderDeserialize(postcard::Error),
    /// Received a reasonable `Header`, but unable to deserialize the body
    /// of the request.
    ResponseBodyDeserialize(postcard::Error),
    /// The client and server disagreed on the protocol version
    ResponseVersionMismatch,
    /// The resposne contained an unexpected Method field.
    ResponseBadMethod,
    /// The response contained an unexpected sequence number.
    ResponseBadSeqno,
    /// The response contained an unexpected type tag [`Key`].
    ResponseKeyMismatch,
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
    /// [`Endpoint`] type `E`, which bundles the request type as `E::Request`,
    /// the response type as `E::Response`, and the hash of the two schemas
    /// and path as the associated const `E::KEY`.
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
    fn send_then_receive_typed_frames<'req, 'resp, E: Endpoint>(
        &'resp mut self,
        req: &E::Request<'req>,
    ) -> Result<Response<E::Response<'resp>>, ClientIoError<<Self::Io as Io>::Error>> {
        let seqno = self.next_sequence_number();
        let (storage, interface) = self.parts();
        let StorageView { rqst_buf, resp_buf } = storage.buffers();

        // SERIALIZE OUTGOING...
        let hdrout = Header {
            method: Method::Request,
            version: 0,
            seqno,
            key: E::KEY,
        };
        let mut out = Serializer {
            output: SerSlice::new(rqst_buf),
        };
        hdrout
            .serialize(&mut out)
            .map_err(ClientError::RequestSerialize)?;
        req.serialize(&mut out)
            .map_err(ClientError::RequestSerialize)?;
        // TODO: CRC? Wrapping flavor?
        let used = out
            .output
            .finalize()
            .map_err(ClientError::RequestSerialize)?;

        // Exchange...
        let recvd = interface
            .send_then_receive_raw_frames(used, resp_buf)
            .map_err(ClientIoError::Io)?;

        // DESERIALIZE INCOMING
        let mut inc = Deserializer::from_flavor(DeSlice::new(recvd));
        let hdrin =
            Header::deserialize(&mut inc).map_err(ClientError::ResponseHeaderDeserialize)?;

        // Check that response header matches all the qualities that we
        // expect...

        if hdrin.version != hdrout.version {
            return Err(ClientError::ResponseVersionMismatch.into());
        }
        if hdrin.method != Method::Response {
            return Err(ClientError::ResponseBadMethod.into());
        }

        // TODO: We need to handle the "wildcard" error if there was some kind
        // of fatal wire error where the server wasn't able to decode our
        // data at all, like with a CRC error or header corruption. In this
        // case, the seqno and key may not match at all.

        if hdrin.seqno != hdrout.seqno {
            return Err(ClientError::ResponseBadSeqno.into());
        }
        if hdrin.key != hdrout.key {
            return Err(ClientError::ResponseKeyMismatch.into());
        }

        // Happy with the header, get the body
        let body =
            E::Response::deserialize(&mut inc).map_err(ClientError::ResponseBodyDeserialize)?;

        // TODO: Ensure all bytes have been consumed? DeSlice::finalize?

        Ok(Response {
            hdr: hdrin,
            resp: body,
        })
    }
}

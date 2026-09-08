use core::fmt::Debug;
use postcard::{
    Serializer,
    ser_flavors::{Flavor as _, Slice as SerSlice},
};
use postcard_schema_ng::key::Key;
use serde::Serialize;

use crate::{
    Request, RequestRaw,
    interface::Endpoint,
    io::{Storage, StorageView},
    wire::{Header, Method},
};

pub struct RawInterfaceFrame<'data, T> {
    /// Interface specific metadata, if any
    pub meta: T,
    pub raw: &'data [u8],
}

#[derive(Debug, PartialEq)]
pub enum ServerError {
    WrongMethod,
    UnknownEndpoint,
    RequestHeaderDeserialize(postcard::Error),
    RequestBodyDeserialize(postcard::Error),
    ResponseSerialize(postcard::Error),
    VersionMismatch,
    BadHeader,
}

#[derive(Debug, PartialEq)]
pub enum ServerInterfaceError<E> {
    Server(ServerError),
    Interface(E),
}

impl<E> From<ServerError> for ServerInterfaceError<E> {
    fn from(value: ServerError) -> Self {
        Self::Server(value)
    }
}

// TODO: "Interface" is an overloaded term. Maybe call this "Wire" or something
// that gets across that this is the "I/O" portion of the backend.
//
// TODO: Right now the client has one send+recv fn that does both back to back
// and doesn't have to expose a metadata param, while the server has split them
// up. Is this good? Do we want to make this consistent? Or document why not being
// consistent is the right choice here.
pub trait Interface {
    /// Metadata, like the peer address for UDP
    type Meta;
    type Error: Debug;

    // TODO: this is for getting a request
    // TODO: do we need some interface specific metadata to capture stuff like
    // the source address for UDP?
    // TODO: do we need a None option for cases where we don't want to send a NAK
    // back the the sending party?
    // TODO: None means "no error" but also "no data"
    fn recv_one_frame_raw<'data>(
        &mut self,
        incoming: &'data mut [u8],
    ) -> Result<Option<RawInterfaceFrame<'data, Self::Meta>>, ServerInterfaceError<Self::Error>>;

    // TODO: this is for sending a response
    //
    // TODO: we can't use RawInterfaceFrame because we don't want the header
    // separate so we can serialize it all together.
    fn send_one_frame_raw(
        &mut self,
        outgoing: RawInterfaceFrame<'_, Self::Meta>,
    ) -> Result<(), ServerInterfaceError<Self::Error>>;

    // TODO: this is for sending an error response. This is the wrong error type.
    // this should only return an error if it is fatal, like "connection lost".
    // Even then, probably not much to do about it.
    //
    // TODO: Option<Header>? this would be for whether we did decode a header at
    // all. Maybe this is two different functions and/or error kinds.
    fn send_one_error(
        &mut self,
        meta: Self::Meta,
        hdr: Option<Header>,
        _err: ServerError,
        resp_buf: &mut [u8],
    ) -> Result<(), ServerInterfaceError<Self::Error>> {
        let hdr = if let Some(mut hdr) = hdr {
            // TODO: wire error type? Reserved names?
            hdr.key = Key::for_path::<()>("error");
            hdr.method = Method::ErrorResponse;
            hdr
        } else {
            Header {
                version: 0,
                method: Method::ErrorResponse,
                seqno: 0,
                key: Key::for_path::<()>("error"),
            }
        };

        let mut out = Serializer {
            output: SerSlice::new(resp_buf),
        };
        hdr.serialize(&mut out)
            .map_err(ServerError::ResponseSerialize)?;
        // TODO: Actually serialize `err` here
        ().serialize(&mut out)
            .map_err(ServerError::ResponseSerialize)?;
        // TODO: CRC? Wrapping flavor?
        let used = out
            .output
            .finalize()
            .map_err(ServerError::ResponseSerialize)?;

        let frame = RawInterfaceFrame { meta, raw: used };

        self.send_one_frame_raw(frame)
    }

    fn serve_one(
        &mut self,
        rqst_buf: &mut [u8],
        resp_buf: &mut [u8],
        func: impl for<'a> FnOnce(RequestRaw<'_>, &'a mut [u8]) -> Result<&'a [u8], ServerError>,
    ) -> Result<(), ServerInterfaceError<Self::Error>> {
        // If we got a fatal wire error, return it with ?
        // If we got no error but no packet, nothing to do
        let Some(frame) = self.recv_one_frame_raw(rqst_buf)? else {
            return Ok(());
        };

        let Ok((hdr, remain)) = postcard::take_from_bytes::<Header>(frame.raw) else {
            return self.send_one_error(frame.meta, None, ServerError::BadHeader, resp_buf);
        };

        // TODO: Should we be checking version and stuff here? process_one does
        // that now
        let rqst_raw = RequestRaw {
            hdr: hdr.clone(),
            rqst: remain,
        };

        let res = (func)(rqst_raw, resp_buf);
        match res {
            Ok(outgoing) => self.send_one_frame_raw(RawInterfaceFrame {
                meta: frame.meta,
                raw: outgoing,
            }),
            Err(e) => self.send_one_error(frame.meta, Some(hdr), e, resp_buf),
        }
    }
}

// TODO: "Backend" isn't a very meaningful name. Come up with a name that better
// gets across that this contains room for ser/de as well as the I/O portion of
// the work.
pub trait Backend {
    type Storage: Storage;
    type Interface: Interface;
    fn parts(&mut self) -> (&mut Self::Storage, &mut Self::Interface);
    fn serve_one(
        &mut self,
        func: impl for<'a> FnOnce(RequestRaw<'_>, &'a mut [u8]) -> Result<&'a [u8], ServerError>,
    ) -> Result<(), ServerInterfaceError<<Self::Interface as Interface>::Error>> {
        let (sto, intfc) = self.parts();
        let StorageView { rqst_buf, resp_buf } = sto.buffers();
        intfc.serve_one(rqst_buf, resp_buf, func)
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

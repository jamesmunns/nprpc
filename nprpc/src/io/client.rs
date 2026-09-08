use postcard::{
    Deserializer, Serializer,
    de_flavors::Slice as DeSlice,
    ser_flavors::{Flavor as _, Slice as SerSlice},
};
use postcard_schema_ng::key::Key;
use serde::{Deserialize, Serialize};

use crate::{
    Error, Response,
    io::{Storage, StorageView},
    wire::{Header, Method},
};

// TODO: "Interface" is an overloaded term. Maybe call this "Wire" or something
// that gets across that this is the "I/O" portion of the backend.
pub trait Interface {
    // TODO: "send reply" is a bad name, we want something that gets across
    // that we are sending a request, then waiting for a reply
    fn send_reply_raw<'a>(
        &mut self,
        outgoing: &[u8],
        incoming: &'a mut [u8],
    ) -> Result<&'a [u8], Error>;
}

// TODO: "Backend" isn't a very meaningful name. Come up with a name that better
// gets across that this contains room for ser/de as well as the I/O portion of
// the work.
pub trait Backend {
    type Storage: Storage;
    type Interface: Interface;
    fn next_sequence_number(&mut self) -> u16;
    fn parts(&mut self) -> (&mut Self::Storage, &mut Self::Interface);

    // TODO: "send reply" is a bad name, we want something that gets across
    // that we are sending a request, then waiting for a reply
    fn send_reply<'de, Q, R>(&'de mut self, key: Key, req: &Q) -> Result<Response<R>, Error>
    where
        Q: Serialize,
        R: Deserialize<'de> + 'de,
    {
        let seqno = self.next_sequence_number();
        let (storage, interface) = self.parts();
        let StorageView { rqst_buf, resp_buf } = storage.buffers();

        // SERIALIZE OUTGOING...
        let hdrout = Header {
            method: Method::Request,
            version: 0,
            seqno,
            key,
        };
        let mut out = Serializer {
            output: SerSlice::new(rqst_buf),
        };
        hdrout.serialize(&mut out).map_err(Error::PostcardSer)?;
        req.serialize(&mut out).map_err(Error::PostcardSer)?;
        // TODO: CRC? Wrapping flavor?
        let used = out.output.finalize().map_err(Error::PostcardSer)?;

        // Exchange...
        let recvd = interface.send_reply_raw(used, resp_buf)?;

        // DESERIALIZE INCOMING
        let mut inc = Deserializer::from_flavor(DeSlice::new(recvd));
        let hdrin = Header::deserialize(&mut inc).map_err(Error::PostcardDeser)?;

        // Check that response header matches all the qualities that we
        // expect...
        //
        // TODO: We need to handle the "wildcard" error if there was some kind
        // of fatal wire error where the server wasn't able to decode our
        // data at all, like with a CRC error or header corruption. In this
        // case, the seqno and key may not match at all.
        if hdrin.version != hdrout.version {
            return Err(Error::VersionMismatch);
        }
        if hdrin.method != Method::Response {
            return Err(Error::BadMethod);
        }
        if hdrin.seqno != hdrout.seqno {
            return Err(Error::BadSeqno);
        }
        if hdrin.key != hdrout.key {
            return Err(Error::KeyMismatch);
        }

        // Happy with the header, get the body
        let body = R::deserialize(&mut inc).map_err(Error::PostcardDeser)?;

        // TODO: Ensure all bytes have been consumed? DeSlice::finalize?

        Ok(Response {
            hdr: hdrin,
            resp: body,
        })
    }
}

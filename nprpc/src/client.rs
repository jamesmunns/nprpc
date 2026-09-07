use postcard::{
    Deserializer, Serializer,
    de_flavors::Slice as DeSlice,
    ser_flavors::{Flavor as _, Slice as SerSlice},
};
use postcard_schema_ng::key::Key;
use serde::{Deserialize, Serialize};

use crate::wire::{Header, Method};
use crate::{Error, Response};

pub trait Storage {
    // TODO: do we want this to be postcard-core flavors?
    fn buffers(&mut self) -> (&mut [u8], &mut [u8]);
}

pub trait Interface {
    fn send_reply_raw<'a>(
        &mut self,
        outgoing: &[u8],
        incoming: &'a mut [u8],
    ) -> Result<&'a [u8], Error>;
}

pub trait Backend {
    type Storage: Storage;
    type Interface: Interface;
    fn next_sequence_number(&mut self) -> u16;
    fn parts(&mut self) -> (&mut Self::Storage, &mut Self::Interface);
    fn send_reply<'de, Q, R>(&'de mut self, key: Key, req: &Q) -> Result<Response<R>, Error>
    where
        Q: Serialize,
        R: Deserialize<'de> + 'de,
    {
        let seqno = self.next_sequence_number();
        let (sto, intfc) = self.parts();
        let (out, inc) = sto.buffers();

        // SERIALIZE OUTGOING...
        let hdrout = Header {
            method: Method::Request,
            version: 0,
            seqno,
            key,
        };
        let mut out = Serializer {
            output: SerSlice::new(out),
        };
        hdrout.serialize(&mut out).map_err(Error::PostcardSer)?;
        req.serialize(&mut out).map_err(Error::PostcardSer)?;

        // Exchange...
        // TODO: CRC? Wrapping flavor?
        let used = out.output.finalize().map_err(Error::PostcardSer)?;

        // DESERIALIZE INCOMING
        let recvd = intfc.send_reply_raw(used, inc)?;
        let mut inc = Deserializer::from_flavor(DeSlice::new(recvd));
        let hdrin = Header::deserialize(&mut inc).map_err(Error::PostcardDeser)?;

        if hdrin.seqno != hdrout.seqno {
            return Err(Error::BadSeqno);
        }
        if hdrin.method != Method::Response {
            return Err(Error::BadMethod);
        }
        if hdrin.version != hdrout.version {
            return Err(Error::VersionMismatch);
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

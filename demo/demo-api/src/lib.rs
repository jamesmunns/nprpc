use nprpc::{autobuffer, compose_interfaces, interface};
use postcard_schema_ng::Schema;
use serde::{Deserialize, Serialize};

/////////////////////////////////////////////////////
// API types
/////////////////////////////////////////////////////

#[derive(Schema, Deserialize, Serialize)]
pub struct Fancy {
    a: u8,         // 1
    b: u16,        // 3
    c: u32,        // 5
    d: u64,        // 10
    e: (i8, i16),  // 4
    f: (i32, i64), // 15
    g: [bool; 8],  // 8
                   // ==
                   // 46 ✔️
}

#[cfg(not(feature = "std"))]
type EightString<'a> = postcard_schema_ng::max_len::MaxLenStr<'a, 8>;

#[cfg(feature = "std")]
use postcard_schema_ng::max_len::{MaxLenStr, MaxLenString};

/////////////////////////////////////////////////////
// Interface definitions
/////////////////////////////////////////////////////

interface! {
    mod ops {
        fn mult_three(u32) -> u32;
    }
}

interface! {
    /// This is a basic interface
    ///
    /// It's good for stuff and things.
    mod basic {
        /// Beep
        fn mult_two(u32) -> u32;
        /// Boop
        fn to_stringa(u32) -> MaxLenString<8>;
        /// Bib
        fn to_stringb(MaxLenString<8>) -> u32;
        /// Bim - when used on a desktop
        #[cfg(feature = "std")]
        fn billy(MaxLenString<8>) -> MaxLenString<8>;
        /// Bim - when used on embedded
        #[cfg(not(feature = "std"))]
        fn billy(MaxLenStr<'req, 8>) -> MaxLenStr<'resp, 8>;
        /// Bap
        fn to_stringd(MaxLenString<8>) -> MaxLenString<8>;
        /// Swoop
        fn is_good(Fancy) -> bool;
        /// Scoop
        fn fancy_boi(Fancy) -> u32;
    }
}

interface! {
    mod two {
        fn one_more_thing(u32) -> bool;
    }
}

compose_interfaces! {
     mod: composite,
     interfaces: [
         crate::basic,
         crate::two,
         crate::ops,
     ]
}

// interface without auto-sizable buffers
interface! {
    mod unsizable {
        fn s2s(String) -> String;
    }
}

pub struct ServerImpl;
use nprpc::Request;

#[cfg(feature = "std")]
impl basic::Server for ServerImpl {
    fn mult_two(&mut self, req: Request<u32>) -> u32 {
        req.req * 2
    }

    fn to_stringa(&mut self, req: Request<u32>) -> MaxLenString<8> {
        req.req.to_string().try_into().unwrap()
    }

    fn to_stringb(&mut self, req: Request<MaxLenString<8>>) -> u32 {
        req.req.len() as u32
    }

    fn to_stringd(&mut self, req: Request<MaxLenString<8>>) -> MaxLenString<8> {
        req.req
    }

    fn billy(&mut self, req: Request<MaxLenString<8>>) -> MaxLenString<8> {
        if req.req.len() > 5 {
            MaxLenString::<8>::try_from(":(").unwrap()
        } else {
            MaxLenString::<8>::try_from(":)").unwrap()
        }
    }

    fn is_good(&mut self, req: Request<Fancy>) -> bool {
        let mut good = true;
        for g in req.req.g {
            good &= g;
        }
        good
    }

    fn fancy_boi(&mut self, req: Request<Fancy>) -> u32 {
        req.req.c * 5
    }
}

impl ops::Server for ServerImpl {
    fn mult_three(&mut self, _req: nprpc::Request<u32>) -> u32 {
        todo!()
    }
}

impl two::Server for ServerImpl {
    fn one_more_thing(&mut self, req: Request<u32>) -> bool {
        req.req.is_multiple_of(2)
    }
}

#[cfg(feature = "std")]

interface! {
    mod borrow {
        fn fancy_boi2(Fancy) -> MaxLenStr<'resp, 8>;
        fn fancy_boi4(MaxLenStr<'req, 8>) -> Fancy;
        fn bypass(MaxLenStr<'req, 8>) -> MaxLenStr<'resp, 8>;
    }
}

autobuffer!(BorrowBuf, borrow);

// TODO: I think the syntax for proxying could work by overriding
// `Server::dispatch_one`, something like:
//
// impl borrow::Server for ServerImpl {
//     proxy! {
//         fn fancy_boi2 -> self.packrat;
//         fn fancy_boi4 -> self.packrat;
//         fn bypass -> self.packrat;
//     }
// }

#[cfg(test)]
mod test {
    use std::ops::Deref;

    use nprpc::{
        RequestRaw, ServerError, autobuffer,
        io::client::{Backend, Io},
        wire::{Header, Method},
    };

    use crate::basic::Client;

    use super::*;

    // Automatically sized buffers for the composite interface
    autobuffer!(CompBuffers, crate::composite);

    struct ServerAsClient {
        #[allow(clippy::type_complexity)]
        inner:
            Box<dyn for<'a> FnMut(RequestRaw<'_>, &'a mut [u8]) -> Result<&'a [u8], ServerError>>,
    }
    impl Io for ServerAsClient {
        type Error = ServerError;
        fn send_then_receive_raw_frames<'a>(
            &mut self,
            outgoing: &[u8],
            incoming: &'a mut [u8],
        ) -> Result<&'a [u8], Self::Error> {
            println!("=> {:?}", outgoing);
            let (hdr, remain) = postcard::take_from_bytes::<Header>(outgoing).unwrap();
            println!("-> {:?}", hdr.key);
            let raw = RequestRaw { hdr, rqst: remain };
            (self.inner)(raw, incoming)
        }
    }

    struct TestClient {
        buf: CompBuffers,
        intfc: ServerAsClient,
        seq: u16,
    }

    impl TestClient {
        /// Silly fake client that implements its "interface" by taking ownership
        /// of the server and shoving the incoming requests into it, then spitting
        /// the responses back out
        #[allow(clippy::type_complexity)]
        pub fn new(
            hdlr: Box<
                dyn for<'a> FnMut(RequestRaw<'_>, &'a mut [u8]) -> Result<&'a [u8], ServerError>,
            >,
        ) -> Self {
            TestClient {
                buf: CompBuffers::new(),
                intfc: ServerAsClient { inner: hdlr },
                seq: 0,
            }
        }
    }

    impl Backend for TestClient {
        type Storage = CompBuffers;
        type Io = ServerAsClient;

        fn next_sequence_number(&mut self) -> u16 {
            let now = self.seq;
            self.seq = self.seq.wrapping_add(1);
            now
        }

        fn parts(&mut self) -> (&mut Self::Storage, &mut Self::Io) {
            let Self { buf, intfc, seq: _ } = self;
            (buf, intfc)
        }
    }

    #[test]
    pub fn exercise_manual() {
        let mut x = ServerImpl;
        let mut cli = TestClient::new(Box::new(move |raw, out| {
            // TODO: if the server errors here, we should reserialize the error
            // and put that back into `out`
            Ok(<ServerImpl as composite::Server>::dispatch_one(&mut x, raw, out).unwrap())
        }));

        let res = cli
            .send_then_receive_typed_frames::<basic::endpoints::mult_two>(&200)
            .unwrap();

        assert_eq!(res.resp, 400u32);
    }

    #[test]
    pub fn exercise() {
        // Make a Server...
        let mut x = ServerImpl;

        // ...then make a client with a Backend that wraps the whole server and
        // just shuttles responses into and out of it (instead of transiting over
        // a wire).
        let mut cli = TestClient::new(Box::new(move |raw, out| {
            Ok(<ServerImpl as composite::Server>::dispatch_one(&mut x, raw, out).unwrap())
        }));

        let res = cli.mult_two(&200).unwrap();
        assert_eq!(res.hdr.method, Method::Response);
        assert_eq!(res.hdr.seqno, 0);
        assert_eq!(res.resp, 400u32);

        let res = cli.to_stringa(&123).unwrap();
        assert_eq!(res.hdr.method, Method::Response);
        assert_eq!(res.hdr.seqno, 1);
        assert_eq!(res.resp.deref(), "123");
    }

    #[test]
    pub fn exercise_borrowed() {
        let mut x = ServerImpl;
        let mut cli = TestClient::new(Box::new(move |raw, out| {
            Ok(<ServerImpl as composite::Server>::dispatch_one(&mut x, raw, out).unwrap())
        }));

        let res = cli
            .send_then_receive_typed_frames::<basic::endpoints::billy>(
                &MaxLenString::<8>::try_from("boop").unwrap(),
            )
            .unwrap();

        assert_eq!(res.resp.deref(), ":)");
    }

    #[test]
    pub fn sizes() {
        println!();
        for k in composite::keys::ALL_KEYS {
            println!("{k:?}");
        }

        const H: usize = Header::SCHEMA.max_size().unwrap();
        const A: usize = composite::info::INTERFACE_INFO.max_request_size.unwrap();
        const B: usize = composite::info::INTERFACE_INFO.max_response_size.unwrap();

        assert_eq!(H, 13);
        assert_eq!(A, 46);
        assert_eq!(B, 9);
        // panic to print...
        // panic!();
    }

    #[test]
    pub fn info() {
        for info in composite::info::ALL_ENDPOINT_INFOS {
            println!("{info:?}");
        }
    }
}

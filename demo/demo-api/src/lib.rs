use nprpc::{compose_interfaces, interface};
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
type EightString = postcard_schema_ng::max_len::MaxLenString<8>;

/////////////////////////////////////////////////////
// Interface definitions
/////////////////////////////////////////////////////

interface! {
     mod: ops,
     | method            | request       | response      |
     | ------            | -------       | --------      |
     | mult_three        | u32           | u32           |
}

#[cfg(not(feature = "std"))]
interface! {
     mod: basic,
     | method     | request             | response          |
     | ------     | -------             | --------          |
     | mult_two   | u32                 | u32               |
     | to_stringa | u32                 | EightString<'b>    |
     | to_stringb | EightString<'a>     | u32               |
     | billy      | EightString<'a>     | EightString<'b>    |
}

#[cfg(feature = "std")]
interface! {
     mod: basic,
     | method     | request     | response      |
     | ------     | -------     | --------      |
     | mult_two   | u32         | u32           |
     | to_stringa | u32         | EightString   |
     | to_stringb | EightString | u32           |
     | billy      | EightString | EightString   |
     | to_stringd | EightString | EightString   |
     | is_good    | Fancy       | bool          |
     | fancy_boi  | Fancy       | u32           |
}

interface! {
     mod: two,
     | method         | request | response |
     | ------         | ------- | -------- |
     | one_more_thing | u32     | bool     |
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
     mod: unsizable,
     | method            | request       | response      |
     | ------            | -------       | --------      |
     | s2s               | String        | String        |
}

pub struct ServerImpl;
use nprpc::Request;

#[cfg(feature = "std")]
impl basic::Server for ServerImpl {
    fn mult_two(&mut self, req: Request<u32>) -> u32 {
        req.req * 2
    }

    fn to_stringa(&mut self, req: Request<u32>) -> EightString {
        req.req.to_string().try_into().unwrap()
    }

    fn to_stringb(&mut self, req: Request<EightString>) -> u32 {
        req.req.len() as u32
    }

    fn to_stringd(&mut self, req: Request<EightString>) -> EightString {
        req.req
    }

    fn billy(&mut self, req: Request<EightString>) -> EightString {
        if req.req.len() > 5 {
            EightString::try_from(":(").unwrap()
        } else {
            EightString::try_from(":)").unwrap()
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
    fn mult_three(&mut self, req: Request<u32>) -> u32 {
        req.req * 3
    }
}

impl two::Server for ServerImpl {
    fn one_more_thing(&mut self, req: Request<u32>) -> bool {
        req.req.is_multiple_of(2)
    }
}

#[cfg(test)]
mod test {
    use std::ops::Deref;

    use nprpc::{Backend, Error, Header, Interface, Method, autobuffer};
    use postcard_schema_ng::key::Key;

    use crate::basic::Client;

    use super::*;

    // Automatically sized buffers for the composite interface
    autobuffer!(CompBuffers, crate::composite);

    struct ServerInterface {
        #[allow(clippy::type_complexity)]
        inner: Box<dyn for<'a> FnMut(&Header, &[u8], &'a mut [u8]) -> Result<&'a [u8], Error>>,
    }
    impl Interface for ServerInterface {
        fn send_reply_raw<'a>(
            &mut self,
            outgoing: &[u8],
            incoming: &'a mut [u8],
        ) -> Result<&'a [u8], Error> {
            println!("=> {:?}", outgoing);
            let (header, remain) = postcard::take_from_bytes::<Header>(outgoing).unwrap();
            println!("-> {:?}", header.key);
            (self.inner)(&header, remain, incoming)
        }
    }

    struct TestClient {
        buf: CompBuffers,
        intfc: ServerInterface,
        seq: u16,
    }

    impl TestClient {
        /// Silly fake client that implements its "interface" by taking ownership
        /// of the server and shoving the incoming requests into it, then spitting
        /// the responses back out
        #[allow(clippy::type_complexity)]
        pub fn new(
            hdlr: Box<dyn for<'a> FnMut(&Header, &[u8], &'a mut [u8]) -> Result<&'a [u8], Error>>,
        ) -> Self {
            TestClient {
                buf: CompBuffers::new(),
                intfc: ServerInterface { inner: hdlr },
                seq: 0,
            }
        }
    }

    impl Backend for TestClient {
        type Storage = CompBuffers;
        type Interface = ServerInterface;

        fn next_sequence_number(&mut self) -> u16 {
            let now = self.seq;
            self.seq = self.seq.wrapping_add(1);
            now
        }

        fn parts(&mut self) -> (&mut Self::Storage, &mut Self::Interface) {
            let Self { buf, intfc, seq: _ } = self;
            (buf, intfc)
        }
    }

    #[test]
    pub fn exercise_manual() {
        let mut x = ServerImpl;
        let mut cli = TestClient::new(Box::new(move |hdr, inc, out| {
            <ServerImpl as composite::Server>::process_one(&mut x, hdr.clone(), inc, out)
        }));

        let res = cli
            .send_reply::<u32, u32>(Key::for_2ty_path::<u32, u32>("basic/mult_two"), &200)
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
        let mut cli = TestClient::new(Box::new(move |hdr, inc, out| {
            <ServerImpl as composite::Server>::process_one(&mut x, hdr.clone(), inc, out)
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
        let mut cli = TestClient::new(Box::new(move |hdr, inc, out| {
            <ServerImpl as composite::Server>::process_one(&mut x, hdr.clone(), inc, out)
        }));

        let res = cli
            .send_reply::<EightString, EightString>(
                Key::for_2ty_path::<EightString, EightString>("basic/billy"),
                &EightString::try_from("boop").unwrap(),
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

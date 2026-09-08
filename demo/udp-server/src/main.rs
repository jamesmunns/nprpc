use std::net::{SocketAddr, UdpSocket};

use nprpc::{
    autobuffer,
    io::{
        Storage, StorageView,
        server::{Backend, RawInterfaceFrame},
    },
    wire::Header,
};
use postcard_schema_ng::max_len::MaxLenString;
use udp_api::composite::{self, Server};

const PORT: u16 = 5501;

// Auto-sized buffer type
autobuffer!(ApiBufs, udp_api::composite);

struct Wire {
    socket: BoundUdpSocket,
    buffers: ApiBufs,
}

struct BoundUdpSocket(UdpSocket);
impl nprpc::io::server::Interface for BoundUdpSocket {
    type Meta = SocketAddr;

    fn recv_one_frame_raw<'data>(
        &mut self,
        incoming: &'data mut [u8],
    ) -> Result<Option<RawInterfaceFrame<'data, Self::Meta>>, nprpc::Error> {
        let Ok((got, peer)) = self.0.recv_from(incoming) else {
            println!("Uhh (connect)...");
            return Ok(None);
        };
        let used = &incoming[..got];
        println!("==(RECV)=> {} ({:02X?})", got, used);
        Ok(Some(RawInterfaceFrame {
            meta: peer,
            raw: used,
        }))
    }

    fn send_one_frame_raw(
        &mut self,
        outgoing: RawInterfaceFrame<'_, Self::Meta>,
    ) -> Result<(), nprpc::Error> {
        // If sending fails oh well, todo maybe log?
        println!("<=(SEND)== {} ({:02X?})", outgoing.raw.len(), outgoing.raw);
        println!();
        let _ = self.0.send_to(outgoing.raw, outgoing.meta);
        Ok(())
    }
}

impl nprpc::io::server::Backend for Wire {
    type Storage = ApiBufs;
    type Interface = BoundUdpSocket;

    fn parts(&mut self) -> (&mut Self::Storage, &mut Self::Interface) {
        let Self { socket, buffers } = self;
        (buffers, socket)
    }
}

fn main() -> std::io::Result<()> {
    let mut wire = Wire {
        socket: BoundUdpSocket(UdpSocket::bind(format!("127.0.0.1:{PORT}"))?),
        buffers: ApiBufs::new(),
    };
    let mut server = ServerImpl { name: None };
    println!("Server Buffer Sizes:");
    println!("  - RQST: {}", wire.buffers.rqst_buf.len());
    println!("  - RESP: {}", wire.buffers.resp_buf.len());
    println!();

    loop {
        let res = wire.serve_one(|reqraw, outgoing| {
            <ServerImpl as composite::Server>::process_one(
                &mut server,
                reqraw.hdr,
                reqraw.rqst,
                outgoing,
            )
        });
        // let StorageView { rqst_buf, resp_buf } = buffers.buffers();
        // let Ok((got, peer)) = socket.recv_from(rqst_buf) else {
        //     println!("Uhh (connect)...");
        //     continue;
        // };
        // let used = &rqst_buf[..got];
        // println!("==(RECV)=> {} ({:02X?})", got, used);
        // // this is gross to *have* to do manually
        // let Ok((hdr, remain)) = postcard::take_from_bytes::<Header>(used) else {
        //     println!("Uhh (bad header)...");
        //     // TODO: reply with error?
        //     continue;
        // };

        // match server.process_one(hdr, remain, resp_buf) {
        //     Ok(reply) => {
        //         println!("<=(SEND)== {} ({:02X?})", reply.len(), reply);
        //         println!();
        //         socket.send_to(reply, peer).unwrap();
        //     }
        //     Err(e) => panic!("{e:?}"),
        // }
    }
}

struct ServerImpl {
    name: Option<MaxLenString<32>>,
}

impl udp_api::hello::Server for ServerImpl {
    fn loopback(&mut self, req: nprpc::Request<u32>) -> u32 {
        println!("hello/loopback: {}", req.req);
        req.req
    }
}

impl udp_api::kv::Server for ServerImpl {
    fn set_name(&mut self, req: nprpc::Request<MaxLenString<32>>) {
        println!("kv/set_name: {}", req.req);
        self.name = Some(req.req);
    }

    fn get_name(&mut self, _req: nprpc::Request<()>) -> Option<MaxLenString<32>> {
        println!("kv/get_name");
        self.name.clone()
    }
}

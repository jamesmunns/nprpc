use std::net::{SocketAddr, UdpSocket};

use nprpc::{
    ServerIoError, autobuffer,
    io::{Storage, server::RawIoFrame},
};
use postcard_schema_ng::max_len::MaxLenString;
use udp_api::composite;

const PORT: u16 = 5501;

// Auto-sized buffer type
autobuffer!(ApiBufs, udp_api::composite);

struct ServerImpl {
    name: Option<MaxLenString<32>>,
}

impl udp_api::hello::Server for ServerImpl {
    fn loopback(&mut self, rqst: nprpc::Request<u32>) -> u32 {
        println!("hello/loopback: {}", rqst.body);
        rqst.body
    }
}

impl udp_api::kv::Server for ServerImpl {
    fn set_name(&mut self, rqst: nprpc::Request<MaxLenString<32>>) {
        println!("kv/set_name: {}", rqst.body);
        self.name = Some(rqst.body);
    }

    fn get_name(&mut self, _req: nprpc::Request<()>) -> Option<MaxLenString<32>> {
        println!("kv/get_name");
        self.name.clone()
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
        let res = <ServerImpl as composite::Server>::serve_one(&mut server, &mut wire);
        if let Err(e) = res {
            println!("Err: {e:?}");
        }
    }
}

/////
// TODO: Items below here should probably be in `nprpc` as standard interface
// impls.
//

struct Wire<S: Storage> {
    socket: BoundUdpSocket,
    buffers: S,
}

struct BoundUdpSocket(UdpSocket);
impl nprpc::io::server::Io for BoundUdpSocket {
    type Error = std::io::Error;
    type Meta = SocketAddr;

    fn recv_one_frame_raw<'data>(
        &mut self,
        incoming: &'data mut [u8],
    ) -> Result<Option<RawIoFrame<'data, Self::Meta>>, ServerIoError<Self::Error>> {
        let (got, peer) = self.0.recv_from(incoming).map_err(ServerIoError::Io)?;
        let used = &incoming[..got];
        println!("==(RECV)=> {} ({:02X?})", got, used);
        Ok(Some(RawIoFrame {
            meta: peer,
            raw: used,
        }))
    }

    fn send_one_frame_raw(
        &mut self,
        outgoing: RawIoFrame<'_, Self::Meta>,
    ) -> Result<(), ServerIoError<Self::Error>> {
        // If sending fails oh well, todo maybe log?
        println!("<=(SEND)== {} ({:02X?})", outgoing.raw.len(), outgoing.raw);
        println!();
        self.0
            .send_to(outgoing.raw, outgoing.meta)
            .map(drop)
            .map_err(ServerIoError::Io)
    }
}

impl<S: Storage> nprpc::io::server::Backend for Wire<S> {
    type Storage = S;
    type Io = BoundUdpSocket;

    fn parts(&mut self) -> (&mut Self::Storage, &mut Self::Io) {
        let Self { socket, buffers } = self;
        (buffers, socket)
    }
}

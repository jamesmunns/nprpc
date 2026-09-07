use std::net::UdpSocket;

use nprpc::{
    autobuffer,
    client::{Storage, StorageView},
    wire::Header,
};
use postcard_schema_ng::max_len::MaxLenString;
use udp_api::composite::Server;

const PORT: u16 = 5501;

// Auto-sized buffer type
autobuffer!(ApiBufs, udp_api::composite);

fn main() -> std::io::Result<()> {
    let socket = UdpSocket::bind(format!("127.0.0.1:{PORT}"))?;
    let mut buffers = ApiBufs::new();
    let mut server = ServerImpl { name: None };
    println!("Server Buffer Sizes:");
    println!("  - RQST: {}", buffers.rqst_buf.len());
    println!("  - RESP: {}", buffers.resp_buf.len());
    println!();

    loop {
        let StorageView { rqst_buf, resp_buf } = buffers.buffers();
        let Ok((got, peer)) = socket.recv_from(rqst_buf) else {
            println!("Uhh (connect)...");
            continue;
        };
        let used = &rqst_buf[..got];
        println!("==(RECV)=> {} ({:02X?})", got, used);
        // this is gross to *have* to do manually
        let Ok((hdr, remain)) = postcard::take_from_bytes::<Header>(used) else {
            println!("Uhh (bad header)...");
            // TODO: reply with error?
            continue;
        };

        match server.process_one(hdr, remain, resp_buf) {
            Ok(reply) => {
                println!("<=(SEND)== {} ({:02X?})", reply.len(), reply);
                println!();
                socket.send_to(reply, peer).unwrap();
            }
            Err(e) => panic!("{e:?}"),
        }
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

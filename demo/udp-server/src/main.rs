use nprpc::{Served, autobuffer};
use nprpc_udp::UdpServer;
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
        *rqst.body
    }
}

impl udp_api::kv::Server for ServerImpl {
    fn set_name(&mut self, rqst: nprpc::Request<MaxLenString<32>>) {
        println!("kv/set_name: {}", rqst.body);
        self.name = Some(rqst.body.clone());
    }

    fn get_name(&mut self, _req: nprpc::Request<()>) -> Option<MaxLenString<32>> {
        println!("kv/get_name");
        self.name.clone()
    }
}

fn main() -> std::io::Result<()> {
    let mut wire = UdpServer::bind(("127.0.0.1", PORT), ApiBufs::new())?;
    let mut server = ServerImpl { name: None };
    println!("Server Buffer Sizes:");
    println!("  - RQST: {}", wire.storage().rqst_buf.len());
    println!("  - RESP: {}", wire.storage().resp_buf.len());
    println!();

    loop {
        match <ServerImpl as composite::Server>::serve_one(&mut server, &mut wire) {
            Ok(Served::Idle | Served::Responded) => {}
            Ok(Served::Rejected(e)) => println!("Rejected: {e:?}"),
            Ok(_) => {}
            Err(e) => println!("Err: {e:?}"),
        }
    }
}

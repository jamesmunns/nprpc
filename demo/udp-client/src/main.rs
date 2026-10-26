use std::time::Duration;

use nprpc::autobuffer;
use nprpc_udp::UdpClient;

use udp_api::hello::Client as _;
use udp_api::kv::Client as _;
autobuffer!(ApiBufs, udp_api::composite);

fn main() {
    let mut client = UdpClient::connect("127.0.0.1:5501".parse().unwrap(), ApiBufs::new())
        .expect("couldn't connect");
    client
        .set_timeout(Some(Duration::from_secs(1)))
        .expect("couldn't set timeout");

    for i in 0..3 {
        let got = client.loopback(&i).unwrap();
        println!("Loopback Success:");
        println!("  - hedr: {:?}", got.hedr);
        println!("  - resp: {}", got.body);

        println!();
        println!("Get name...");
        let name = client.get_name(&()).unwrap();
        println!("  - resp: {:?}", name.body);

        let new_name = format!("Server {i}").try_into().unwrap();
        println!("Set name ({new_name:?})...");
        let _ = client.set_name(&new_name).unwrap();

        std::thread::sleep(Duration::from_secs(1));
    }
}

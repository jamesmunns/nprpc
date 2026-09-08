use std::{net::UdpSocket, num::Wrapping, time::Duration};

use nprpc::{
    autobuffer,
    io::{
        Storage,
        client::{Backend, Interface},
    },
};

// TODO: compose clients
use udp_api::hello::Client as _;
use udp_api::kv::Client as _;
autobuffer!(ApiBufs, udp_api::composite);

fn main() {
    let socket = UdpSocket::bind("127.0.0.1:3400").expect("couldn't bind to address");
    socket
        .connect("127.0.0.1:5501")
        .expect("connect function failed");

    let mut client = MyClient {
        buf: ApiBufs::new(),
        socket: ConnectedUdpSocket(socket),
        ctr: Wrapping(0),
    };

    for i in 0..3 {
        let got = client.loopback(&i).unwrap();
        println!("Loopback Success:");
        println!("  - hdr:  {:?}", got.hdr);
        println!("  - resp: {}", got.resp);

        println!();
        println!("Get name...");
        let name = client.get_name(&()).unwrap();
        println!("  - resp: {:?}", name.resp);

        let new_name = format!("Server {i}").try_into().unwrap();
        println!("Set name ({new_name:?})...");
        let _ = client.set_name(&new_name).unwrap();

        std::thread::sleep(Duration::from_secs(1));
    }
}

/////
// TODO: Items below here should probably be in `nprpc` as standard interface
// impls.
//
struct ConnectedUdpSocket(UdpSocket);

impl Interface for ConnectedUdpSocket {
    fn send_reply_raw<'a>(
        &mut self,
        outgoing: &[u8],
        incoming: &'a mut [u8],
    ) -> Result<&'a [u8], nprpc::Error> {
        // TODO: these needs some kind of interface-specific error type
        self.0.send(outgoing).unwrap();
        let used = self.0.recv(incoming).unwrap();
        Ok(&incoming[..used])
    }
}

struct MyClient<S: Storage> {
    ctr: Wrapping<u16>,
    buf: S,
    socket: ConnectedUdpSocket,
}

impl<S: Storage> Backend for MyClient<S> {
    type Storage = S;
    type Interface = ConnectedUdpSocket;

    fn next_sequence_number(&mut self) -> u16 {
        let now = self.ctr;
        self.ctr += 1;
        now.0
    }

    fn parts(&mut self) -> (&mut Self::Storage, &mut Self::Interface) {
        let Self {
            ctr: _,
            buf,
            socket,
        } = self;
        (buf, socket)
    }
}

use std::{net::UdpSocket, num::Wrapping};

use nprpc::{
    autobuffer,
    client::{Backend, Interface},
};
use udp_api::hello::Client;

autobuffer!(ApiBufs, udp_api::hello);

struct ConnectedUdpSocket(UdpSocket);

struct MyClient {
    ctr: Wrapping<u16>,
    buf: ApiBufs,
    socket: ConnectedUdpSocket,
}

// This could live in nprpc? Requires already connected I think, so maybe need
// a wrapper type? Or we should use send_to?
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

impl Backend for MyClient {
    type Storage = ApiBufs;

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
        println!("Success:");
        println!("  - hdr:  {:?}", got.hdr);
        println!("  - resp: {}", got.resp)
    }
}

//! Clients and servers talking over localhost

use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};

use nprpc::{
    ClientError, ClientIoError, Request, Served, autobuffer, interface,
    interface::Method,
    io::client::Backend as _,
    wire::{HEADER_LEN, Operation, WireError},
};
use nprpc_udp::{UdpClient, UdpServer, VecStorage, is_timeout};

interface! {
    mod example {
        fn double(u32) -> u32;
    }
}

interface! {
    mod other {
        fn negate(i32) -> i32;
    }
}

autobuffer!(Bufs, example);
autobuffer!(OtherBufs, other);

use example::Client as _;
use other::Client as _;

struct Doubler;

impl example::Server for Doubler {
    fn double(&mut self, rqst: Request<u32>) -> u32 {
        *rqst.body * 2
    }
}

/// Serves `example` on localhost until dropped.
struct TestServer {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<Vec<Served>>>,
}

impl TestServer {
    fn start() -> Self {
        let mut udp = UdpServer::bind("127.0.0.1:0", Bufs::new()).unwrap();
        udp.set_timeout(Some(Duration::from_millis(10))).unwrap();
        let addr = udp.socket().local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let thread = std::thread::spawn({
            let stop = stop.clone();
            move || {
                let mut served = vec![];
                while !stop.load(Ordering::Relaxed) {
                    match <Doubler as example::Server>::serve_one(&mut Doubler, &mut udp).unwrap() {
                        Served::Idle => {}
                        s => served.push(s),
                    }
                }
                served
            }
        });
        Self {
            addr,
            stop,
            thread: Some(thread),
        }
    }

    /// Stops the server, returning what it served (other than idling).
    fn stop(mut self) -> Vec<Served> {
        self.stop.store(true, Ordering::Relaxed);
        self.thread.take().unwrap().join().unwrap()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn client<S: nprpc::io::Storage>(server: &TestServer, storage: S) -> UdpClient<S> {
    let client = UdpClient::connect(server.addr, storage).unwrap();
    client.set_timeout(Some(Duration::from_secs(5))).unwrap();
    client
}

#[test]
fn typed() {
    let server = TestServer::start();
    let mut client = client(&server, Bufs::new());

    for (seqno, n) in [3u32, 100, u32::MAX / 2].into_iter().enumerate() {
        let resp = client.double(&n).unwrap();
        assert_eq!(resp.hedr.op, Operation::Response);
        assert_eq!(usize::from(resp.hedr.seqno), seqno);
        assert_eq!(resp.body, n * 2);
    }

    assert!(matches!(
        server.stop()[..],
        [Served::Responded, Served::Responded, Served::Responded]
    ));
}

#[test]
fn raw_with_vec_storage() {
    let server = TestServer::start();
    let mut client = client(&server, VecStorage::new(HEADER_LEN + 8, 64));

    let resp = client
        .send_then_receive_raw(<example::methods::double as Method>::KEY, &[21])
        .unwrap();
    assert_eq!(resp.hedr.op, Operation::Response);
    assert_eq!(resp.body, [42]);
}

#[test]
fn unknown_method() {
    let server = TestServer::start();
    let mut client = client(&server, OtherBufs::new());

    assert!(matches!(
        client.negate(&1),
        Err(ClientIoError::Client(ClientError::Server(
            WireError::SERVER_UNKNOWN_METHOD
        )))
    ));
    assert!(matches!(
        server.stop()[..],
        [Served::Rejected(nprpc::ServerError::UnknownMethod)]
    ));
}

#[test]
fn client_timeout() {
    // Bound, but nothing ever replies
    let silent = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let mut client = UdpClient::connect(silent.local_addr().unwrap(), Bufs::new()).unwrap();
    client.set_timeout(Some(Duration::from_millis(10))).unwrap();

    match client.double(&1) {
        Err(ClientIoError::Io(e)) => assert!(is_timeout(&e), "{e:?}"),
        r => panic!("expected a timeout, got {r:?}"),
    }
}

#[test]
fn server_idle() {
    let mut udp = UdpServer::bind("127.0.0.1:0", Bufs::new()).unwrap();
    udp.set_timeout(Some(Duration::from_millis(10))).unwrap();
    assert!(matches!(
        <Doubler as example::Server>::serve_one(&mut Doubler, &mut udp),
        Ok(Served::Idle)
    ));
}

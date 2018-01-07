#[cfg(feature="logger")]
extern crate env_logger;
#[macro_use]
extern crate log;
extern crate futures;
extern crate tokio_core;
extern crate tokio_timer;
extern crate recon_link;

use std::io;
use std::net::SocketAddr;
use std::time::Duration;
use futures::{Async, AsyncSink, Poll, StartSend};
use futures::future::Future;
use futures::stream::{self, Stream};
use futures::sink::{Sink};
use tokio_core::reactor::{Core, Handle};
use tokio_core::net::TcpStream;
use tokio_timer::Timer;
use recon_link::conn::{Connection, Message, NewTransport};
use recon_link::framing::{FramedLineTransport, new_line_transport, ReconFrame};

pub type MessageContent = ReconFrame;

#[cfg(feature="logger")]
mod logging {
    pub fn init_logger() {
        use env_logger;
        env_logger::init().unwrap();
    }
}

#[cfg(not(feature="logger"))]
mod logging {
    pub fn init_logger() {}
}

fn main() {
    logging::init_logger();

    info!("hello, world!");

    let timer = Timer::default();
    let mut core = Core::new().unwrap();
    let handle = core.handle();

    let addr = "127.0.0.1:6666".parse().unwrap();

    let stream = stream::iter_ok((1..1000).map(|n| ReconFrame::Message(format!("{}", n))))
        .and_then(|value| {
            debug!("next value is {:?}", value);
            timer.sleep(Duration::from_millis(500))
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
                .map(|_| {
                    debug!("producing delayed value {:?}", value);
                    value
                })
        });

    let conn = Connection::new(core.handle(), stream, PrinterSink, NewTcpTransport(addr, handle));

    core.run(conn).unwrap();
}

struct PrinterSink;

impl Sink for PrinterSink {
    type SinkItem = Message<MessageContent>;
    type SinkError = io::Error;

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        println!("{:?}", item);
        Ok(AsyncSink::Ready)
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        Ok(Async::Ready(()))
    }
}

#[derive(Clone)]
struct NewTcpTransport(SocketAddr, Handle);

impl NewTransport for NewTcpTransport {
    type Transport = FramedLineTransport<TcpStream>;
    type Error = io::Error;
    type Future = Box<Future<Item=Self::Transport,Error=Self::Error>>;

    fn new_transport(&self) -> Self::Future {
        Box::new(TcpStream::connect(&self.0, &self.1).map(|stream| new_line_transport(stream))) as Self::Future
    }
}
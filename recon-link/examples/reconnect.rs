#[cfg(feature="logger")]
extern crate env_logger;
#[macro_use]
extern crate log;
extern crate futures;
extern crate tokio_core;
extern crate tokio_timer;
extern crate recon_link;

use std::time::Duration;
use futures::future::Future;
use futures::stream::{self, Stream};
use futures::sync::mpsc::{channel};
use tokio_core::reactor::Core;
use tokio_timer::Timer;
use recon_link::conn::{Config, Connection};
use recon_link::transport::{NewTcpLineTransport};
use recon_link::framing::Frame;
use recon_link::errors::*;

pub type NetworkMessage = Frame;

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

    let stream = stream::iter_ok((1..1000).map(|n| Frame::Message(format!("{}", n))))
        .and_then(|value| {
            debug!("next value is {:?}", value);
            timer.sleep(Duration::from_millis(500))
                .map_err(|e| Error::with_chain(e, "timer error"))
                .map(|_| {
                    debug!("producing delayed value {:?}", value);
                    value
                })
        });

    let (tx, rx) = channel(0);

    let printer = rx.map(|i| println!("{:?}", i)).collect().map(|_| ());
    handle.spawn(printer);

    let config = Config {
        outbound_max: 10,
        inbound_max: 10,
        outbound_max_age: Duration::from_millis(1000)
    };

    let conn = Connection::new(core.handle(), stream, tx, NewTcpLineTransport { addr, handle }, config);

    let result = core.run(conn);

    println!("event loop terminated: {:?}", result);
}

#[cfg(feature="logger")]
extern crate env_logger;
#[macro_use]
extern crate log;
extern crate futures;
extern crate tokio_core;
extern crate tokio_timer;
extern crate recon_link;

use std::io;
use std::time::Duration;
use futures::future::Future;
use futures::stream::{self, Stream};
use tokio_core::reactor::Core;
use tokio_timer::Timer;
use recon_link::conn::Connection;

fn init_logger() {
    if cfg!(feature="logger") {
        env_logger::init().unwrap();
    }
}

fn main() {
    init_logger();

    info!("hello, world!");

    let timer = Timer::default();
    let mut core = Core::new().unwrap();

    let addr = "127.0.0.1:6666".parse().unwrap();

    let stream = stream::iter_ok((1..1000).map(|n| format!("{}\n", n)))
        .and_then(|value| {
            debug!("next value is {}", value);
            timer.sleep(Duration::from_millis(500))
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
                .map(|_| {
                    debug!("producing delayed value {}", value);
                    value
                })
        });

    let conn = Connection::new(addr, core.handle(), stream);

    core.run(conn).unwrap();
}
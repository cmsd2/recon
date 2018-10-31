#[cfg(feature="logger")]
extern crate env_logger;
#[macro_use]
extern crate log;
extern crate futures;
extern crate tokio_core;
extern crate tokio_timer;
extern crate serde_json;
extern crate recon_link;

use std::time::Duration;
use futures::future::{self, Future};
use futures::stream::{self, Stream};
use futures::sink::Sink;
use futures::sync::mpsc::{channel};
use tokio_core::reactor::Core;
use tokio_timer::Timer;
use recon_link::transport::{NewTcpLineTransport};
use recon_link::framing::Frame;
use recon_link::link::{Command, Config, Link};
use recon_link::errors::*;

pub type NetworkMessage = Frame;

#[cfg(feature="logger")]
mod logging {
    pub fn init_logger() {
        use env_logger;
        env_logger::init().unwrap();
        println!("logger");
    }
}

#[cfg(not(feature="logger"))]
mod logging {
    pub fn init_logger() {println!("no logger")}
}

fn main() {
    logging::init_logger();

    info!("hello, world!");

    let timer = Timer::default();
    let mut core = Core::new().unwrap();
    let handle = core.handle();

    let addr = "127.0.0.1:6666".parse().unwrap();

    let mut link = Link::new(handle.clone(), "1".to_owned(), Config { inbound_max: 1, outbound_max: 1, buffering: 0 }).expect("error creating link");
    let tx = link.sender();
    
    let stream = stream::iter_ok((1..1000).map(|n| serde_json::to_value(n).expect("serde error") ))
        .and_then(move |value| {
            debug!("next value is {:?}", value);
            timer.sleep(Duration::from_millis(500))
                .map_err(|e| Error::with_chain(e, "timer error"))
                .map(|_| {
                    debug!("producing delayed value {:?}", value);
                    value
                })
        }).and_then(move |value| {
            debug!("sending value to link");
            tx.clone().send(Command::Broadcast { message: value })
                .map_err(|e| Error::with_chain(e, "channel send error"))
        }).collect()
        .map(|_| ())
        .map_err(|e| panic!("error from stream: {:?}", e));

    handle.spawn(stream);

    link.add_connection("2".to_owned(), addr).expect("add connection error");
    
    core.run(link.for_each(|item| {
        println!("item: {:?}", item);
        future::ok(())
    })).expect("error running link");
}

#[cfg(feature="logger")]
extern crate env_logger;
#[macro_use]
extern crate log;
extern crate futures;
extern crate tokio_core;
extern crate recon_util;

use std::io;
use futures::Future;
use futures::sink::Sink;
use futures::stream::Stream;
use futures::sync::mpsc::{channel};
use tokio_core::reactor::Core;
use recon_util::pub_sub::{PubSub,Subscribable,Subscription};

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

pub type Message = String;

pub fn main() {
    println!("hello, world!");
    logging::init_logger();
    info!("info");

    let pub_sub = PubSub::<Message>::new(0);
    let (tx,rx) = channel(0);
    let _subscription: Subscription<_> = pub_sub.subscribe(tx).expect("subscribe error");

    let mut core = Core::new().unwrap();
    let handle = core.handle();

    let fut = rx
        .take(1)
        .collect()
        .map_err(|()| io::Error::new(io::ErrorKind::Other, format!("take error")));
    
    handle.spawn(pub_sub
        .sender()
        .send("hello".into())
        .map_err(|e| panic!(e))
        .map(|_sender| ())
    );

    handle.spawn(pub_sub
        .map_err(|e| panic!("pub sub error: {}", e))
    );

    let result = core.run(fut);

    println!("event loop terminated: {:?}", result);
}
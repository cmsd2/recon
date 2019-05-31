#[macro_use]
extern crate log;
extern crate actix;
extern crate actix_web;
extern crate bytes;
extern crate env_logger;
extern crate futures;
extern crate serde;
extern crate serde_json;
extern crate tokio_codec;
extern crate tokio_core;
extern crate tokio_io;
extern crate tokio_retry;
extern crate tokio_tcp;
#[macro_use]
extern crate serde_derive;
extern crate snowflake;
extern crate failure;

use actix::prelude::*;
use std::net::SocketAddr;
use tokio_tcp::TcpListener;

pub mod codec;
pub mod connection_manager;
pub mod connection_table;
pub mod link;
pub mod tcp_server;
pub mod web;

fn main() {
    env_logger::try_init().expect("error initialising env_logger");

    info!("Hello, world from {:?}!", module_path!());

    let system = System::new("hello");

    // let connections_table = ConnectionTable::new().start();

    let addr: SocketAddr = "127.0.0.1:6666".parse().expect("error parsing socket addr");
    let listener = TcpListener::bind(&addr).unwrap();
    let server_addr = tcp_server::TcpServer::new(listener);

    let web_addr: SocketAddr = "127.0.0.1:8080".parse().expect("error parsing socket addr");
    web::create_server(web::Options {
        connections: server_addr,
        listen: web_addr,
    }).expect("error starting admin server");

    match system.run() {
        Ok(()) => {},
        Err(err) => {
            error!("error: {}", err);
            std::process::exit(-1);
        }
    }
}

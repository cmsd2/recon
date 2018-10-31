#[macro_use]
extern crate log;
extern crate env_logger;
extern crate futures;
extern crate bytes;
extern crate tokio_core;
extern crate tokio_io;
extern crate tokio_codec;
extern crate tokio_tcp;
extern crate actix;
extern crate actix_web;
extern crate serde;
extern crate serde_json;
#[macro_use]
extern crate serde_derive;
extern crate snowflake;

use std::net::SocketAddr;
use tokio_tcp::{TcpListener};
use actix::prelude::*;
use connection_table::{AddListener,ConnectionTable};

pub mod tcp_server;
pub mod codec;
pub mod link;
pub mod connection_table;
pub mod web;

fn main() {
    env_logger::try_init().expect("error initialising env_logger");

    info!("Hello, world from {:?}!", module_path!());

    let system = System::new("hello");

    let connections_table = ConnectionTable::new().start();
    let web_addr: SocketAddr = "127.0.0.1:8080".parse().expect("error parsing socket addr");
    web::create_server(web::Options { 
        connections: connections_table.clone(), 
        listen: web_addr,
    }).expect("error starting admin server");

    let addr: SocketAddr = "127.0.0.1:6666".parse().expect("error parsing socket addr");
    let listener = TcpListener::bind(&addr).unwrap();
    let server_addr = tcp_server::TcpServer::new(listener, connections_table);
    
    let code = system.run();

    std::process::exit(code);
}

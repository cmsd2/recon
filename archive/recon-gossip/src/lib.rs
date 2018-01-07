#![allow(unused_imports)]

#[macro_use]
extern crate serde_derive;

extern crate serde;
extern crate serde_json;

extern crate rand;
extern crate chrono;
extern crate futures;
extern crate futures_mpsc;
extern crate tokio_core;
extern crate tokio_service;
extern crate tokio_proto;
extern crate tokio_timer;
extern crate tokio_io;
extern crate bincode;
extern crate rustc_serialize;
#[macro_use]
extern crate quick_error;
#[macro_use]
extern crate log;
extern crate bytes;
extern crate bytes_more;

use std::env;
use std::io;
use std::net::SocketAddr;
use std::io::{Read, Write};

use futures::Future;
use futures::stream::Stream;
use tokio_core::net::{TcpStream, TcpStreamNew};
use tokio_core::reactor::Core;
use tokio_io::io::{copy};

pub mod framing;
pub mod service;
pub mod client;
pub mod fut;
pub mod switchboard;
pub mod watchdog;
pub mod conn;
pub mod link;
pub mod multiplex;
pub mod upb;
pub mod lpb;

mod serde_types;
pub use serde_types::*;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
    }
}

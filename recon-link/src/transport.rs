use std;
use std::net::SocketAddr;
use futures::future::Future;
use tokio_core::reactor::Handle;
use tokio_core::net::TcpStream;
use framing::{FramedLineTransport,new_line_transport};
use errors::*;

pub trait NewTransport {
    type Future: Future<Item=Self::Transport,Error=Self::Error>;
    type Transport;
    type Error: std::error::Error + Send + 'static;

    fn new_transport(&self) -> Self::Future;
}

#[derive(Clone)]
pub struct NewTcpLineTransport {
    pub addr: SocketAddr,
    pub handle: Handle,
}

impl NewTcpLineTransport {
    pub fn new(addr: SocketAddr, handle: Handle) -> NewTcpLineTransport {
        NewTcpLineTransport {
            addr: addr,
            handle: handle,
        }
    }
}

impl NewTransport for NewTcpLineTransport {
    type Transport = FramedLineTransport<TcpStream>;
    type Error = Error;
    type Future = Box<Future<Item=Self::Transport,Error=Self::Error>>;

    fn new_transport(&self) -> Self::Future {
        Box::new(TcpStream::connect(&self.addr, &self.handle)
            .map(|stream| new_line_transport(stream))
            .map_err(|e| Error::with_chain(e, "error establishing network connection"))) as Self::Future
    }
}

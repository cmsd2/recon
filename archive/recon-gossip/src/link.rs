
use std::io;
use std::time::Duration;
use std::net::SocketAddr;
use tokio_core::reactor::Handle;
use tokio_core::net::TcpStream;
use futures::{Async, Poll, Future, Stream};
use futures::sync::mpsc::{channel, Sender, Receiver};

use ::client::{NewClient, Client, LineClient, NewLineClient};
use ::framing::{ReconFrame, FramedLineTransport, new_line_transport};
use ::service::{AsyncService, Funnel, ServerHandle, serve};
use ::conn::Conn;
use ::switchboard::{Switchboard, ClientImpl};
use ::{LinkMessage};

pub enum Command {
    SendMessage(LinkMessage),
    AddConnection{ id: String, addr: SocketAddr },
    RemoveConnection{ id: String },
}

pub struct Link {
    switchboard: Switchboard,
    channel_tx: Sender<Command>,
    channel_rx: Receiver<Command>,
    server_rx: Receiver<String>,
    server_handle: ServerHandle,
}

impl Link {
    pub fn new<F>(handle: Handle, id: String, addr: SocketAddr, conn_factory: F) -> io::Result<Link> 
            where F: NewClient<Request=String,Error=io::Error,Item=ClientImpl,Io=TcpStream> + Clone + 'static {
        
        let (server_tx, server_rx) = try!(channel(&handle));
        let (tx, rx) = try!(channel(&handle));
        let server_handle = try!(serve(&handle, addr, Funnel::new(server_tx)));

        Ok(Link {
            switchboard: try!(Switchboard::new(handle, id, conn_factory)),
            channel_tx: tx,
            channel_rx: rx,
            server_rx: server_rx,
            server_handle: server_handle,
        })
    }

    pub fn channel(&self) -> Sender<Command> {
        self.channel_tx.clone()
    }

    fn handle(&mut self, cmd: Command) -> io::Result<()> {
        match cmd {
            Command::SendMessage(m) => self.switchboard.send(m),
            Command::AddConnection{id, addr} => self.switchboard.add(id, addr),
            Command::RemoveConnection{id} => self.switchboard.remove(id),
        }
    }

    pub fn add<S>(&self, id: S, addr: SocketAddr) -> io::Result<()> where S: Into<String> {
        self.switchboard.add(id.into(), addr)
    }

    pub fn remove(&self, id: &str) -> io::Result<()> {
        self.switchboard.remove(id)
    }

    pub fn send(&self, msg: LinkMessage) -> io::Result<()> {
        self.switchboard.send(msg)
    }
}

impl Stream for Link {
    type Item=String;
    type Error=io::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        try!(self.switchboard.poll());

        while let Async::Ready(Some(cmd)) = try!(self.channel_rx.poll()) {
            try!(self.handle(cmd))
        }

        self.server_rx.poll()
    }
}
use std::sync::{Arc, Mutex};
use std::cell::RefCell;
use std::io;
use std::collections::{VecDeque,HashMap};
use std::net::SocketAddr;
use std::time::Duration;
use tokio_core::reactor::Handle;
use tokio_core::net::TcpStream;
use futures::{self, Future, Poll, Async, Stream};
use futures::sync::mpsc::{channel, Sender, Receiver};
use serde_json;

use super::client::{NewClient, Client, LineClient, NewLineClient};
use super::framing::FramedLineTransport;
use super::fut::*;
use super::service::*;
use super::conn::*;
use super::{LinkMessage};

pub type ClientImpl = Client<LineClient<FramedLineTransport<TcpStream>>>;
pub type ConnImpl = Conn<ClientImpl>;
pub type ConnFactory = NewClient<Request=String,Error=io::Error,Item=ClientImpl,Io=TcpStream>;

pub struct SwitchboardConnection {
    conn: ConnImpl,
    pub sender: Sender<String>,
}

impl SwitchboardConnection {
    pub fn new(conn: ConnImpl) -> SwitchboardConnection {
        SwitchboardConnection {
            sender: conn.channel(),
            conn: conn,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub to: Option<String>,
    pub body: String,
}

impl Message {
    pub fn new<S,T>(to: S, body: T) -> Message where S: Into<Option<String>>, T: Into<String> {
        Message {
            to: to.into(),
            body: body.into(),
        }
    }
}

pub enum Command {
    SendMessage(LinkMessage),
    AddConnection{ id: String, addr: SocketAddr },
    RemoveConnection{ id: String },
}

pub struct Switchboard {
    id: String,
    handle: Handle,
    connections: HashMap<String, SwitchboardConnection>,
    channel_tx: Sender<Command>,
    channel_rx: Receiver<Command>,
    conn_factory: Box<NewClient<Request=String,Error=io::Error,Item=ClientImpl,Io=TcpStream>>,
}

impl Switchboard {
    pub fn new<F>(handle: Handle, id: String, conn_factory: F) -> io::Result<Switchboard> where F: NewClient<Request=String,Error=io::Error,Item=ClientImpl,Io=TcpStream> + Clone + 'static {
        let (tx, rx) = try!(channel(&handle));

        Ok(Switchboard {
            id: id,
            handle: handle,
            connections: HashMap::new(),
            channel_tx: tx,
            channel_rx: rx,
            conn_factory: Box::new(conn_factory),
        })
    }

    pub fn channel(&self) -> Sender<Command> {
        self.channel_tx.clone()
    }

    pub fn connection_ids(&self) -> Vec<String> {
        self.connections.keys().map(|s| s.to_owned()).collect()
    }

    pub fn do_broadcast(&self, msg: LinkMessage) -> io::Result<()> {
        let json_msg = try!(self.encode(&msg));

        for c in self.connections.values() {
            try!(c.sender.send(json_msg.clone()));
        }

        Ok(())
    }

    pub fn send(&self, msg: LinkMessage) -> io::Result<()> {
        self.channel_tx.send(Command::SendMessage(msg))
    }

    fn do_send(&self, msg: LinkMessage) -> io::Result<()> {
        if let Some(ref to) = msg.to {
            match self.connections.get(to) {
                Some(conn) => {
                    try!(conn.sender.send(try!(self.encode(&msg))));
                },
                None => {
                    debug!("no connection registered with switchboard id {} for msg {:?}", to, msg);
                }
            }
        } else {
            try!(self.do_broadcast(msg));
        }

        Ok(())
    }

    fn encode(&self, msg: &LinkMessage) -> io::Result<String> {
        serde_json::to_string(&msg).map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidInput, "json encoding error")
        })
    }

    pub fn add<S>(&self, client_id: S, addr: SocketAddr) -> io::Result<()> where S: Into<String> {
        self.channel_tx.send(Command::AddConnection{id: client_id.into(), addr: addr})
    }

    fn do_add(&mut self, client_id: String, addr: SocketAddr) -> io::Result<()> {
        let conn = try!(Conn::new(self.handle.clone(), addr, self.conn_factory.clone()));

        self.connections.insert(client_id.into(), SwitchboardConnection::new(conn));

        Ok(())
    }

    pub fn remove<S>(&self, client_id: S) -> io::Result<()> where S: Into<String> {
        self.channel_tx.send(Command::RemoveConnection{id: client_id.into()})
    }

    fn do_remove(&mut self, client_id: &str) -> io::Result<()> {
        self.connections.remove(client_id);

        Ok(())
    }

    fn handle(&mut self, command: Command) -> io::Result<()> {
        match command {
            Command::SendMessage(msg) => try!(self.do_send(msg)),
            Command::AddConnection{id, addr} => try!(self.do_add(id, addr)),
            Command::RemoveConnection{id} => try!(self.do_remove(&id)),
        }

        Ok(())
    }
}

impl Future for Switchboard {
    type Item=();
    type Error=io::Error;

    fn poll(&mut self) -> Poll<(), io::Error> {
        while let Async::Ready(Some(command)) = try!(self.channel_rx.poll()) {
            try!(self.handle(command));
        }

        for mut c in self.connections.values_mut() {
            try!(c.conn.poll());
        }

        Ok(Async::NotReady)
    }
}
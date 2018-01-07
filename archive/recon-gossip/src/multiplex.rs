
use std::io;
use std::time::Duration;
use std::collections::HashMap;
use std::net::SocketAddr;
use tokio_core::reactor::Handle;
use tokio_core::net::TcpStream;
use futures::{Async, Poll, Future, Stream};
use futures::sync::mpsc::{channel, Sender, Receiver};
use serde_json::{self, Value};

use ::client::{NewClient, Client, LineClient, NewLineClient};
use ::framing::{ReconFrame, FramedLineTransport, new_line_transport};
use ::service::{AsyncService, Funnel, serve};
use ::conn::Conn;
use ::switchboard::{self, Switchboard, ClientImpl};
use ::link::{Link};
use ::{LinkMessage, MultiplexMessage};

#[derive(Debug, Clone)]
pub struct Message {
    pub to: Option<String>,
    pub from: String,
    pub key: String,
    pub body: Value,
}

impl Message {
    pub fn new<M>(to: Option<String>, from: String, module: M, body: Value) -> Message 
            where M: Into<String> {
        Message {
            to: to,
            from: from,
            key: module.into(),
            body: body,
        }
    }

}

pub enum Command {
    Subscribe {key: String, sender: Sender<Message> },
    Unsubscribe {key: String},
    SendMessage {msg: Message},
    AddConnection{ id: String, addr: SocketAddr },
    RemoveConnection{ id: String },
}

pub struct MultiplexLink {
    id: String,
    link: Link,
    subscribers: HashMap<String, Sender<Message>>,
    channel_tx: Sender<Command>,
    channel_rx: Receiver<Command>,
}

impl MultiplexLink {
    pub fn new<F>(handle: Handle, id: String, addr: SocketAddr, conn_factory: F) -> io::Result<MultiplexLink> 
            where F: NewClient<Request=String,Error=io::Error,Item=ClientImpl,Io=TcpStream> + Clone + 'static {
        
        let (tx, rx) = try!(channel(&handle));

        Ok(MultiplexLink {
            id: id.clone(),
            link: try!(Link::new(handle, id, addr, conn_factory)),
            subscribers: HashMap::new(),
            channel_tx: tx,
            channel_rx: rx,
        })
    }

    pub fn channel(&self) -> Sender<Command> {
        self.channel_tx.clone()
    }

    // TODO: do encoding and decoding better

    fn encode(&self, msg: Message) -> io::Result<LinkMessage> {
        let mm = serde_json::to_value(MultiplexMessage::new(msg.key, msg.body));
        Ok(LinkMessage::new(msg.to, msg.from, mm))
    }

    fn decode<'a>(&self, msg: &'a str) -> io::Result<Message> {
        let lm: LinkMessage = try!(serde_json::from_str(msg)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, "json from_value error")));
        let mm: MultiplexMessage = try!(serde_json::from_value(lm.body)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, "json from_value error")));

        Ok(Message::new(lm.to, lm.from, mm.key, mm.body))
    }
    
    fn dispatch(&mut self, msg: String) -> io::Result<()> {
        let msg = try!(self.decode(&msg));

        if let Some(chan) = self.subscribers.get(&msg.key) {
            chan.send(msg)
        } else {
            debug!("unhandle message {:?}", msg);

            Ok(())
        }
    }

    pub fn send(&self, msg: Message) -> io::Result<()> {
        self.do_send(msg)
    }

    fn do_send(&self, msg: Message) -> io::Result<()> {
        let encoded_msg = try!(self.encode(msg));

        self.link.send(encoded_msg)
    }

    pub fn subscribe<S>(&self, key: S, chan: Sender<Message>) -> io::Result<()> where S: Into<String> {
        self.channel_tx.send(Command::Subscribe{key: key.into(), sender: chan})
    }

    fn do_subscribe(&mut self, key: String, chan: Sender<Message>) -> io::Result<()> {
        self.subscribers.insert(key, chan);
        Ok(())
    }

    pub fn unsubscribe<S>(&self, key: S) -> io::Result<()> where S: Into<String> {
        self.channel_tx.send(Command::Unsubscribe{key: key.into()})
    }

    fn do_unsubscribe(&mut self, key: String) -> io::Result<()> {
        self.subscribers.remove(&key);
        Ok(())
    }

    pub fn add<S>(&self, id: S, addr: SocketAddr) -> io::Result<()> where S: Into<String> {
        self.link.add(id.into(), addr)
    }

    pub fn remove(&self, id: &str) -> io::Result<()> {
        self.link.remove(id)
    }

    fn handle(&mut self, cmd: Command) -> io::Result<()> {
        match cmd {
            Command::SendMessage {msg} => self.do_send(msg),
            Command::Subscribe {key, sender} => self.do_subscribe(key, sender),
            Command::Unsubscribe {key} => self.do_unsubscribe(key),
            Command::AddConnection{id, addr} => self.add(id, addr),
            Command::RemoveConnection{id} => self.remove(&id),
        }
    }
}

impl Future for MultiplexLink {
    type Item=();
    type Error=io::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        while let Async::Ready(Some(msg)) = try!(self.link.poll()) {
            try!(self.dispatch(msg));
        }

        while let Async::Ready(Some(cmd)) = try!(self.channel_rx.poll()) {
            try!(self.handle(cmd));
        }

        Ok(Async::NotReady)
    }
}
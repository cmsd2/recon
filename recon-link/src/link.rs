use std::result;
use std::collections::{BTreeMap,VecDeque,BTreeSet};
use std::net::SocketAddr;
use std::time::Duration;
use futures::{Async, AsyncSink, Poll};
use futures::future::Future;
use futures::sink::Sink;
use futures::stream::Stream;
use futures::sync::mpsc::{channel, Sender, Receiver};
use tokio_core::reactor::Handle;
use recon_util::codec::Codec;
use errors::*;
use conn;
use transport;
use framing;

pub type Id = String;
pub type EncodedMessage = String;
pub type TransportMessage = framing::Frame;
pub type Transport = transport::NewTcpLineTransport;

struct ConnectionImpl<M> {
    pub fut: Box<Future<Item=(),Error=Error>>,
    pub tx: Sender<M>,
    pub rx: Receiver<conn::Message<M>>,
}

pub enum Command<M> {
    AddConnection {
        id: Id,
        addr: SocketAddr,
    },
    Broadcast {
        message: M,
    },
    Unicast {
        recipient: Id,
        message: M,
    }
}

struct OutboundMessage<O> {
    pub connection_id: Id,
    pub encoded_message: O,
}

pub struct Config {
    pub outbound_max: usize,
    pub buffering: usize,
}

pub struct Link<M, C, E> where C: Codec<Item=M,EncodedItem=EncodedMessage,Error=E>, E: Into<Error> {
    handle: Handle,
    pub id: Id,
    tx: Sender<Command<M>>,
    rx: Receiver<Command<M>>,
    connections: BTreeMap<String, ConnectionImpl<TransportMessage>>,
    outbound: VecDeque<OutboundMessage<TransportMessage>>,
    outbound_max: usize,
    inflight: BTreeSet<Id>,
    codec: C,
}

impl <M, C, E> Link<M, C, E> where C: Codec<Item=M,EncodedItem=EncodedMessage,Error=E>, E: Into<Error> {
    pub fn new(handle: Handle, id: String, codec: C, config: Config) -> Link<M,C,E> {
        let (tx, rx) = channel(config.buffering);
        Link {
            handle: handle,
            tx: tx,
            rx: rx,
            id: id,
            connections: BTreeMap::new(),
            outbound: VecDeque::new(),
            outbound_max: config.outbound_max,
            inflight: BTreeSet::new(),
            codec: codec,
        }
    }

    pub fn sender(&self) -> Sender<Command<M>> {
        self.tx.clone()
    }

    pub fn connection_ids(&self) -> Vec<String> {
        self.connections.keys().map(|s| s.to_owned()).collect()
    }

    pub fn ready(&self) -> bool {
        return self.outbound.len() < self.outbound_max;
    }

    pub fn unicast(&mut self, recipient: &Id, msg: M) -> Result<AsyncSink<M>> {
        if !self.ready() {
            return Ok(AsyncSink::NotReady(msg));
        }

        let encoded_msg = try!(self.encode(msg));

        self.outbound.push_back(OutboundMessage { 
            connection_id: recipient.to_owned(), 
            encoded_message: encoded_msg.clone() 
        });

        Ok(AsyncSink::Ready)
    }

    pub fn broadcast(&mut self, msg: M) -> Result<AsyncSink<M>> {
        if !self.ready() {
            return Ok(AsyncSink::NotReady(msg));
        }

        let encoded_msg = try!(self.encode(msg));

        for id in self.connections.keys() {
            self.outbound.push_back(OutboundMessage { 
                connection_id: id.to_owned(), 
                encoded_message: encoded_msg.clone() 
            });
        }

        Ok(AsyncSink::Ready)
    }

    pub fn add_connection(&mut self, id: Id, addr: SocketAddr) -> Result<()> {
        debug!("received command add_connection {} to {}", id, addr);
        let (tx_outbound,rx_outbound) = channel(0);
        let (tx_inbound,rx_inbound) = channel(0);
        
        let config = conn::Config {
            outbound_max: 1,
            outbound_max_age: Duration::from_millis(2000),
            inbound_max: 1,
        };
        
        let new_transport = Transport::new(addr, self.handle.clone());
        let conn = Box::new(conn::Connection::new(
            self.handle.clone(),
            rx_outbound.map_err(|()| Error::from_kind(ErrorKind::ReceiveError)),
            tx_inbound,
            new_transport,
            config
        )) as Box<Future<Item=(),Error=Error>>;
        
        self.connections.insert(id, ConnectionImpl {
            fut: conn,
            tx: tx_outbound,
            rx: rx_inbound,
        });

        Ok(())
    }

    pub fn encode(&self, msg: M) -> Result<TransportMessage> {
        self.codec.encode(msg).map_err(|e| e.into()).map(|msg| framing::Frame::Message(msg))
    }

    fn send_all_outbound(&mut self) -> Poll<(),Error> {
        while let Some(OutboundMessage { connection_id, encoded_message }) = self.outbound.pop_front() {
            match try!(self.send_one_outbound(&connection_id, encoded_message)) {
                AsyncSink::Ready => {
                    self.inflight.insert(connection_id.clone());
                },
                AsyncSink::NotReady(encoded_message) => {
                    self.outbound.push_front(OutboundMessage { connection_id, encoded_message });
                    return Ok(Async::NotReady);
                }
            }
        }

        Ok(Async::Ready(()))
    }

    fn send_one_outbound(&mut self, id: &Id, msg: TransportMessage) -> Result<AsyncSink<TransportMessage>> {
        if let Some(ref mut conn) = self.connections.get_mut(id) {
            conn.tx.start_send(msg).map_err(|e| Error::with_chain(e, "error sending message to connection"))
        } else {
            // TODO: should we raise an error here or not?
            info!("dropping message destined for {}. not connection", id);
            Ok(AsyncSink::Ready)
        }
    }

    fn poll_inflight(&mut self) -> Result<Async<()>> {
        let mut new_inflight = BTreeSet::new();

        for id in self.inflight.iter() {
            if let Some(ref mut conn) = self.connections.get_mut(id) {
                match try!(conn.tx.poll_complete().map_err(|e| Error::with_chain(e, "error waiting for message delivery to connection"))) {
                    Async::Ready(()) => {},
                    Async::NotReady => {
                        new_inflight.insert(id.to_owned());
                    }
                }
            }
        }

        for (ref _id, ref mut conn) in self.connections.iter_mut() {
            try!(conn.tx.poll_complete().map_err(|e| Error::with_chain(e, "error waiting for delivery to connections")));
        }

        self.inflight = new_inflight;

        if self.inflight.is_empty() {
            Ok(Async::Ready(()))
        } else {
            Ok(Async::NotReady)
        }
    }

    fn handle_command(&mut self, command: Command<M>) -> Result<()> {
        match command {
            Command::AddConnection { id, addr } => {
                debug!("received add connection command {} {}", id, addr);
                try!(self.add_connection(id, addr));
            },
            Command::Broadcast { message } => {
                debug!("received command broadcast message");
                match try!(self.broadcast(message)) {
                    AsyncSink::NotReady(_message) => {
                        panic!("tried to queue message when not ready");
                    },
                    AsyncSink::Ready => {}
                }
            },
            Command::Unicast { recipient, message } => {
                debug!("received command unicast message to {}", recipient);
                match try!(self.unicast(&recipient, message)) {
                    AsyncSink::NotReady(_message) => {
                        panic!("tried to queue message when not ready");
                    },
                    AsyncSink::Ready => {}
                }
            }
        }

        Ok(())
    }
}

impl <M,C,E> Future for Link<M,C,E> where C: Codec<Item=M,EncodedItem=EncodedMessage,Error=E>, E: Into<Error> {
    type Item = ();
    type Error = Error;

    fn poll(&mut self) -> result::Result<Async<Self::Item>,Self::Error> {
        if self.ready() {
            match try!(self.rx.poll().map_err(|()| Error::from_kind(ErrorKind::ReceiveError))) {
                Async::Ready(Some(command)) => {
                    try!(self.handle_command(command));
                },
                Async::Ready(None) => {
                    debug!("received end of stream");
                    return Ok(Async::Ready(()));
                },
                Async::NotReady => {}
            }
        }

        try!(self.send_all_outbound());

        try!(self.poll_inflight());

        Ok(Async::NotReady)
    }
}

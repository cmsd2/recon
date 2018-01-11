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
use serde_json;
use recon_util::codec::Codec;
use errors::*;
use conn;
use transport;
use framing;
use proto::LinkMessage;

pub type Id = String;
pub type Message = serde_json::Value;
pub type TransportMessage = framing::Frame;
pub type Transport = transport::NewTcpLineTransport;

struct ConnectionImpl<M> {
    pub fut: Box<Future<Item=(),Error=Error>>,
    pub tx: Sender<M>,
    pub rx: Receiver<conn::Message<M>>,
}

pub enum Command {
    AddConnection {
        id: Id,
        addr: SocketAddr,
    },
    Broadcast {
        message: Message,
    },
    Unicast {
        recipient: Id,
        message: Message,
    }
}

struct OutboundMessage<M> {
    pub connection_id: Id,
    pub encoded_message: M,
}

pub struct Config {
    pub outbound_max: usize,
    pub buffering: usize,
}

pub struct Link {
    handle: Handle,
    pub id: Id,
    tx: Sender<Command>,
    rx: Receiver<Command>,
    connections: BTreeMap<String, ConnectionImpl<TransportMessage>>,
    outbound: VecDeque<OutboundMessage<TransportMessage>>,
    outbound_max: usize,
    inflight: BTreeSet<Id>,
}

impl Link {
    pub fn new(handle: Handle, id: String, config: Config) -> Link {
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
        }
    }

    pub fn sender(&self) -> Sender<Command> {
        self.tx.clone()
    }

    pub fn connection_ids(&self) -> Vec<String> {
        self.connections.keys().map(|s| s.to_owned()).collect()
    }

    pub fn ready(&self) -> bool {
        return self.outbound.len() < self.outbound_max;
    }

    pub fn unicast(&mut self, recipient: &Id, msg: Message) -> Result<AsyncSink<Message>> {
        if !self.ready() {
            return Ok(AsyncSink::NotReady(msg));
        }

        trace!("sending unicast message to {}", recipient);

        let encoded_msg = try!(self.encode(msg));

        self.outbound.push_back(OutboundMessage { 
            connection_id: recipient.to_owned(), 
            encoded_message: encoded_msg.clone() 
        });

        Ok(AsyncSink::Ready)
    }

    pub fn broadcast(&mut self, msg: Message) -> Result<AsyncSink<Message>> {
        if !self.ready() {
            return Ok(AsyncSink::NotReady(msg));
        }

        trace!("sending broadcast message");

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

    pub fn encode(&self, message: Message) -> Result<TransportMessage> {
        let link_message = LinkMessage {
            from: self.id.clone(),
            body: message,
        };
        let encoded_message = serde_json::to_string(&link_message)
            .chain_err(|| "error serialising link message")?;
        Ok(framing::Frame::Message(encoded_message))
    }

    fn send_all_outbound(&mut self) -> Poll<(),Error> {
        trace!("trying to send {} outbound link messages", self.outbound.len());

        while let Some(OutboundMessage { connection_id, encoded_message }) = self.outbound.pop_front() {
            match try!(self.send_one_outbound(&connection_id, encoded_message)) {
                AsyncSink::Ready => {
                    debug!("connection accepted outbound message");
                    self.inflight.insert(connection_id.clone());
                },
                AsyncSink::NotReady(encoded_message) => {
                    debug!("connection not ready for outbound message");
                    self.outbound.push_front(OutboundMessage { connection_id, encoded_message });
                    return Ok(Async::NotReady);
                }
            }
        }

        Ok(Async::Ready(()))
    }

    fn send_one_outbound(&mut self, id: &Id, msg: TransportMessage) -> Result<AsyncSink<TransportMessage>> {
        if let Some(ref mut conn) = self.connections.get_mut(id) {
            trace!("sending link message to connection {}", id);
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
                trace!("polling connection {}", id);
                match try!(conn.tx.poll_complete().map_err(|e| Error::with_chain(e, "error waiting for message delivery to connection"))) {
                    Async::Ready(()) => {},
                    Async::NotReady => {
                        new_inflight.insert(id.to_owned());
                    }
                }
            }
        }

        for (ref id, ref mut conn) in self.connections.iter_mut() {
            trace!("polling connection {}", id);
            try!(conn.fut.poll().map_err(|e| Error::with_chain(e, "error waiting for delivery to connections")));
        }

        self.inflight = new_inflight;

        trace!("{} link messages in flight", self.inflight.len());

        if self.inflight.is_empty() {
            Ok(Async::Ready(()))
        } else {
            Ok(Async::NotReady)
        }
    }

    fn handle_command(&mut self, command: Command) -> Result<()> {
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

impl Future for Link {
    type Item = ();
    type Error = Error;

    fn poll(&mut self) -> result::Result<Async<Self::Item>,Self::Error> {
        trace!("link poll called");

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

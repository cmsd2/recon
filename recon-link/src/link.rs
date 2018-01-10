use std::io;
use std::result;
use std::collections::{BTreeMap,VecDeque,BTreeSet};
use futures::{Async, AsyncSink, Poll};
use futures::future::Future;
use futures::sink::Sink;
use futures::stream::Stream;
use futures::sync::mpsc::{channel, Sender, Receiver};
use tokio_core::reactor::Handle;
use recon_util::codec::Codec;

pub type Id = String;
pub type OutboundError = io::Error;
pub type ConnectionImpl<O> = Box<Sink<SinkItem=O,SinkError=OutboundError>>;
pub type Result<I> = io::Result<I>;
pub type Error = io::Error;

pub enum Command<M> {
    AddConnection {
        id: Id,

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

pub struct Link<M, O, C, E> where C: Codec<Item=M,EncodedItem=O,Error=E>, E: Into<Error>, O: Clone {
    _handle: Handle,
    pub id: Id,
    tx: Sender<Command<M>>,
    rx: Receiver<Command<M>>,
    connections: BTreeMap<String, ConnectionImpl<O>>,
    outbound: VecDeque<OutboundMessage<O>>,
    outbound_max: usize,
    inflight: BTreeSet<Id>,
    codec: C,
}

impl <M, O, C, E> Link<M, O, C, E> where C: Codec<Item=M,EncodedItem=O,Error=E>, E: Into<Error>, O: Clone {
    pub fn new(handle: Handle, id: String, queue: usize, codec: C, buffering: usize) -> Link<M,O,C,E> {
        let (tx, rx) = channel(buffering);
        Link {
            _handle: handle,
            tx: tx,
            rx: rx,
            id: id,
            connections: BTreeMap::new(),
            outbound: VecDeque::new(),
            outbound_max: queue,
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

    pub fn encode(&self, msg: M) -> Result<O> {
        self.codec.encode(msg).map_err(|e| e.into())
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

    fn send_one_outbound(&mut self, id: &Id, msg: O) -> Result<AsyncSink<O>> {
        if let Some(ref mut conn) = self.connections.get_mut(id) {
            conn.start_send(msg)
        } else {
            Err(io::Error::new(io::ErrorKind::Other, format!("no such connection with id {}", id)))
        }
    }

    fn poll_inflight(&mut self) -> Result<Async<()>> {
        let mut new_inflight = BTreeSet::new();

        for id in self.inflight.iter() {
            if let Some(ref mut conn) = self.connections.get_mut(id) {
                match try!(conn.poll_complete()) {
                    Async::Ready(()) => {},
                    Async::NotReady => {
                        new_inflight.insert(id.to_owned());
                    }
                }
            }
        }

        for (ref _id, ref mut conn) in self.connections.iter_mut() {
            try!(conn.poll_complete());
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
            Command::AddConnection { id } => {
                debug!("received command add_connection {}", id);
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

impl <M,O,C,E> Future for Link<M,O,C,E> where C: Codec<Item=M,EncodedItem=O,Error=E>, E: Into<Error>, O: Clone {
    type Item = ();
    type Error = Error;

    fn poll(&mut self) -> result::Result<Async<Self::Item>,Self::Error> {
        if self.ready() {
            match try!(self.rx.poll().map_err(|()| io::Error::new(io::ErrorKind::Other, "receiver poll error"))) {
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

use std::io;
use std::collections::{BTreeMap,VecDeque,BTreeSet};
use futures::{Async, AsyncSink, Poll, StartSend};
use futures::future::Future;
use futures::sink::Sink;
use tokio_core::reactor::Handle;
use codec::Codec;

pub type Id = String;
pub type OutboundError = io::Error;
pub type ConnectionImpl<O> = Box<Sink<SinkItem=O,SinkError=OutboundError>>;
pub type Result<I> = io::Result<I>;
pub type Error = io::Error;

pub enum Command<M> {
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
    connections: BTreeMap<String, ConnectionImpl<O>>,
    outbound: VecDeque<OutboundMessage<O>>,
    outbound_max: usize,
    inflight: BTreeSet<Id>,
    codec: C,
}

impl <M, O, C, E> Link<M, O, C, E> where C: Codec<Item=M,EncodedItem=O,Error=E>, E: Into<Error>, O: Clone {
    pub fn new(handle: Handle, id: String, queue: usize, codec: C) -> Link<M,O,C,E> {
        Link {
            _handle: handle,
            id: id,
            connections: BTreeMap::new(),
            outbound: VecDeque::new(),
            outbound_max: queue,
            inflight: BTreeSet::new(),
            codec: codec,
        }
    }

    pub fn connection_ids(&self) -> Vec<String> {
        self.connections.keys().map(|s| s.to_owned()).collect()
    }

    pub fn unicast(&mut self, recipient: &Id, msg: M) -> Result<AsyncSink<M>> {
        if self.outbound.len() >= self.outbound_max {
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
        if self.outbound.len() >= self.outbound_max {
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
}

impl <M,O,C,E> Sink for Link<M,O,C,E> where C: Codec<Item=M,EncodedItem=O,Error=E>, E: Into<Error>, O: Clone {
    type SinkItem = Command<M>;
    type SinkError = Error;

    fn start_send(
        &mut self, 
        item: Self::SinkItem
    ) -> StartSend<Self::SinkItem, Self::SinkError> {
        match item {
            Command::Broadcast { message } => {
                let result = try!(self.broadcast(message));
                Ok(result.map(|message| Command::Broadcast { message }))
            },
            Command::Unicast { recipient, message } => {
                let result = try!(self.unicast(&recipient, message));
                Ok(result.map(|message| Command::Unicast { recipient, message }))
            }
        }
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        try!(self.send_all_outbound());

        self.poll_inflight()
    }
}
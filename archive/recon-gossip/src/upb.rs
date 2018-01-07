use std::sync::{Arc, Mutex};
use std::cell::RefCell;
use std::io;
use std::collections::{VecDeque,HashMap,HashSet};
use std::net::SocketAddr;
use std::marker::PhantomData;
use tokio_core::reactor::Handle;
use tokio_core::net::TcpStream;
use futures::{self, Future, Poll, Async, Stream};
use futures::sync::mpsc::{channel, Sender, Receiver};
use serde_json;
use serde::{Serialize, Deserialize};
use serde::de::DeserializeOwned;
use rand;
use chrono::{DateTime, Duration, Utc};

use super::client::{NewClient, Client, LineClient, NewLineClient};
use super::framing::FramedLineTransport;
use super::fut::*;
use super::service::*;
use super::conn::*;
use super::{LinkMessage, UpbMessage};
use super::multiplex;

pub type NodeId = String;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MessageId(pub NodeId, pub u64);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Delivery {
    pub message_id: MessageId,
    pub delivered_at: DateTime<Utc>,
}

impl Delivery {
    pub fn new(sender: NodeId, seq: u64) -> Delivery {
        Delivery {
            message_id: MessageId(sender, seq),
            delivered_at: Utc::now(),
        }
    }
}

pub enum Command<T> where T: Serialize + DeserializeOwned + Clone + Send + 'static {
    Broadcast { msg: T }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Message<T> where T: Serialize + DeserializeOwned + Clone + Send + 'static {
    pub from: NodeId,
    pub seq: u64,
    pub value: T,
}

pub struct UnreliableProbabilisticBroadcast<T> where T: Serialize + DeserializeOwned + Clone + Send + 'static {
    id: NodeId,
    delivered: HashSet<Delivery>,
    delivered_gc_age: Duration,
    peers: HashSet<NodeId>,
    link: Sender<multiplex::Command>,
    k: usize,
    multiplex_key: String,
    link_rx: Receiver<multiplex::Message>,
    command_tx: Sender<Command<T>>,
    command_rx: Receiver<Command<T>>,
    rounds: u32,
    pending: VecDeque<Message<T>>,
    seq: u64,
}

impl <T> UnreliableProbabilisticBroadcast<T> where T: Serialize + DeserializeOwned + Clone + Send + 'static {
    pub fn new(handle: Handle, id: NodeId, peers: HashSet<NodeId>, 
            link: Sender<multiplex::Command>, k: usize, rounds: u32, multiplex_key: String, delivered_gc_age: Duration) -> io::Result<UnreliableProbabilisticBroadcast<T>> {
        let (tx, rx) = try!(channel(&handle));
        let (command_tx, command_rx) = try!(channel(&handle));

        try!(link.send(multiplex::Command::Subscribe { key: multiplex_key.clone(), sender: tx }));

        Ok(UnreliableProbabilisticBroadcast {
            id: id,
            delivered: HashSet::new(),
            delivered_gc_age: delivered_gc_age,
            peers: peers,
            k: k,
            multiplex_key: multiplex_key,
            link: link,
            link_rx: rx,
            command_tx: command_tx,
            command_rx: command_rx,
            rounds: rounds,
            pending: VecDeque::new(),
            seq: 0,
        })
    }

    pub fn channel(&self) -> Sender<Command<T>> {
        self.command_tx.clone()
    }

    fn decode_link_message(&self, link_msg: multiplex::Message) -> io::Result<UpbMessage> {
        let gm: UpbMessage = try!(serde_json::from_value(link_msg.body)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, "json decoding error")));
        
        Ok(gm)
    }

    fn handle_broadcast(&mut self, from: String, seq: u64, value: serde_json::Value, ttl: u32) -> io::Result<()> {
        let msg_id = Delivery::new(from.clone(), seq);

        if ttl > 0 {
            try!(self.gossip(UpbMessage::Broadcast { from: from.clone(), seq: seq, value: value.clone(), ttl: ttl - 1 }));
        }

        if self.delivered.insert(msg_id.clone()) {
            let typed_value = try!(serde_json::from_value(value)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, "json decoding error")));

            self.pending.push_back(Message {
                from: from,
                seq: seq,
                value: typed_value
            });
        }

        Ok(())
    }

    fn gossip(&self, gm: UpbMessage) -> io::Result<()> {
        debug!("gossipping to {} targets: {:?}", self.k, gm);

        let gm_value = serde_json::to_value(gm.clone());

        for t in self.picktargets(self.k) {
            debug!("gossipping {:?} to {}", gm, t);

            let mm = multiplex::Message::new(Some(t.to_owned()), self.id.clone(), self.multiplex_key.clone(), gm_value.clone());

            try!(self.link.send(multiplex::Command::SendMessage {msg: mm}));
        }

        Ok(())
    } 

    pub fn broadcast(&self, value: T) -> io::Result<()> {
        self.command_tx.send(Command::Broadcast {msg: value})
    }

    fn do_broadcast(&mut self, msg: T) -> io::Result<()> {
        let value = serde_json::to_value(msg.clone());
        let msg_seq = self.next_msg_seq();

        self.delivered.insert(Delivery::new(self.id.clone(), msg_seq));
        self.pending.push_back(Message {
            from: self.id.clone(),
            seq: msg_seq,
            value: msg
        });
        
        let gm = UpbMessage::Broadcast { from: self.id.clone(), seq: msg_seq, value: value, ttl: self.rounds - 1 };

        self.gossip(gm)
    }

    pub fn picktargets<'a>(&'a self, k: usize) -> Vec<&'a NodeId> {
        let mut rng = rand::thread_rng();
        let candidates = self.peers.iter()
            .filter(|i| *i != &self.id);
        rand::sample(&mut rng, candidates, k)
    }

    fn next_msg_seq(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }

    fn delivered_gc(&mut self) {
        let mut keep = HashSet::new();

        for msg_id in self.delivered.drain() {
            if msg_id.delivered_at + self.delivered_gc_age > Utc::now() {
                keep.insert(msg_id);
            } 
        }

        self.delivered = keep;
    }

    pub fn set_delivered_gc_age(&mut self, age: Duration) {
        self.delivered_gc_age = age;
    }

    pub fn get_delivered_gc_age(&self) -> Duration {
        self.delivered_gc_age
    }
}

impl <T> Stream for UnreliableProbabilisticBroadcast<T> where T: Serialize + DeserializeOwned + Clone + Send + 'static {
    type Item=Message<T>;
    type Error=io::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        while let Async::Ready(Some(cmd)) = try!(self.command_rx.poll()) {
            match cmd {
                Command::Broadcast {msg} => {
                    try!(self.do_broadcast(msg));
                }
            }
        }

        while let Async::Ready(Some(link_msg)) = try!(self.link_rx.poll()) {
            debug!("gossip received link message: {:?}", link_msg);

            let gm = try!(self.decode_link_message(link_msg));
            
            match gm {
                UpbMessage::Broadcast {from, seq, value, ttl} => {
                    try!(self.handle_broadcast(from, seq, value, ttl));
                }
            }
        }

        self.delivered_gc();

        Ok(match self.pending.pop_front() {
            Some(v) => Async::Ready(Some(v)),
            None => Async::NotReady, 
        })
    }
}
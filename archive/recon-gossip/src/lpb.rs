use std::sync::{Arc, Mutex};
use std::cell::RefCell;
use std::io;
use std::collections::{VecDeque,HashMap,HashSet};
use std::net::SocketAddr;
use std::marker::PhantomData;
use tokio_core::reactor::Handle;
use tokio_core::net::TcpStream;
use tokio_timer::Timer;
use futures::{self, Future, Poll, Async, Stream};
use futures::sync::mpsc::{channel, Sender, Receiver};
use serde_json;
use serde::{Serialize, Deserialize};
use serde::de::DeserializeOwned;
use rand::{self, Rng};
use chrono::*;

use super::client::{NewClient, Client, LineClient, NewLineClient};
use super::framing::FramedLineTransport;
use super::fut::*;
use super::service::*;
use super::conn::*;
use super::{LinkMessage, LpbMessage, LpbLinkMessage, UpbMessage};
use super::multiplex;
use super::upb::{self, NodeId, MessageId, UnreliableProbabilisticBroadcast};

/// The output type of this broadcast algorithm
#[derive(Debug, Clone, PartialEq)]
pub struct Message<T> where T: Serialize + DeserializeOwned + Clone + Send + 'static { 
    from: NodeId,
    seq: u64, 
    value: T
}

/// Input commands
pub enum Command<T> where T: Serialize + DeserializeOwned + Clone + Send + 'static {
    Broadcast { msg: T }
}

pub struct LazyProbabilisticBroadcast<T> where T: Serialize + DeserializeOwned + Clone + Send + 'static {
    id: NodeId,
    peers: HashSet<NodeId>,
    link: Sender<multiplex::Command>,
    k: usize, // fan-out factor
    rounds: u32, // message ttl
    alpha: f32, // probability of storing message
    delta: Duration,
    multiplex_key: String,
    link_rx: Receiver<multiplex::Message>,
    command_tx: Sender<Command<T>>,
    command_rx: Receiver<Command<T>>,
    to_deliver: VecDeque<Message<T>>,
    pending: HashMap<MessageId, Message<T>>,
    seq: u64, // lpb's message sequence number
    stored: HashMap<MessageId, Message<T>>,
    next_seq: HashMap<NodeId, u64>, // sequence numbers for peers
    upb: UnreliableProbabilisticBroadcast<LpbMessage>,
    upb_tx: Sender<upb::Command<LpbMessage>>,
    timers: Option<Vec<Box<Future<Item=MessageId, Error=io::Error>>>>,
}

impl <T> LazyProbabilisticBroadcast<T> where T: Serialize + DeserializeOwned + Clone + Send + 'static {
    pub fn new(handle: Handle, id: NodeId, peers: HashSet<NodeId>, 
            link: Sender<multiplex::Command>, k: usize, rounds: u32, alpha: f32, delta: Duration, multiplex_key: String, upb_gc_age: Duration) -> io::Result<LazyProbabilisticBroadcast<T>> {
        let (tx, rx) = try!(channel(&handle));
        let (command_tx, command_rx) = try!(channel(&handle));

        try!(link.send(multiplex::Command::Subscribe { key: multiplex_key.clone(), sender: tx }));

        let upb_impl = try!(UnreliableProbabilisticBroadcast::new(
            handle.clone(),
            id.clone(),
            peers.clone(),
            link.clone(),
            k, rounds,
            format!("{}/upb", multiplex_key.clone()),
            upb_gc_age
        ));

        let upb_tx = upb_impl.channel();

        Ok(LazyProbabilisticBroadcast {
            id: id,
            peers: peers,
            k: k,
            rounds: rounds,
            alpha: alpha,
            delta: delta,
            multiplex_key: multiplex_key,
            link: link,
            link_rx: rx,
            command_tx: command_tx,
            command_rx: command_rx,
            to_deliver: VecDeque::new(),
            pending: HashMap::new(),
            seq: 0,
            stored: HashMap::new(),
            next_seq: HashMap::new(),
            upb: upb_impl,
            upb_tx: upb_tx,
            timers: Some(vec![]),
        })
    }

    pub fn channel(&self) -> Sender<Command<T>> {
        self.command_tx.clone()
    }

    fn gossip(&self, gm: LpbLinkMessage) -> io::Result<()> {
        debug!("gossipping to {} targets: {:?}", self.k, gm);

        for t in self.picktargets(self.k) {
            debug!("gossipping {:?} to {}", gm, t);

            try!(self.send(t.to_owned(), gm.clone()));
        }

        Ok(())
    } 

    pub fn broadcast(&self, value: T) -> io::Result<()> {
        self.command_tx.send(Command::Broadcast {msg: value})
    }

    fn do_broadcast(&mut self, msg: T) -> io::Result<()> {
        let value = serde_json::to_value(msg);
        let msg_seq = self.next_msg_seq();
        let lm = LpbMessage::Data { seq: msg_seq, value: value };
        
        self.upb_tx.send(upb::Command::Broadcast { msg: lm })
    }

    fn handle_upb_broadcast(&mut self, from: String, seq: u64, value: serde_json::Value, ttl: u32) -> io::Result<()> {
        let lpb_msg = try!(serde_json::from_value(value)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, "json decoding error")));

        match lpb_msg {
            LpbMessage::Data { seq: lpb_seq, value: lpb_value } => {
                try!(self.handle_upb_data(from, seq, lpb_seq, lpb_value));
            }
        }

        Ok(())
    }

    fn handle_upb_data(&mut self, from: String, upb_seq: u64, lpb_seq: u64, value: serde_json::Value) -> io::Result<()> {
        let msg_id = MessageId(from.clone(), lpb_seq);
        let typed_value: T = try!(serde_json::from_value(value)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, "json decoding error")));

        let dm = Message {
            from: from.clone(),
            seq: lpb_seq,
            value: typed_value.clone(),
        };        

        let mut rng = rand::thread_rng();
        if rng.gen_range(0f32, 1f32) > self.alpha {
            self.stored.insert(msg_id.clone(), dm.clone());
        }

        let next_seq = self.next_seq.entry(from.clone()).or_insert(1).to_owned();
        if lpb_seq == next_seq {
            self.next_seq.insert(from.clone(), next_seq + 1);

            self.to_deliver.push_back(dm);
        } else if lpb_seq > next_seq {
            self.pending.insert(msg_id.clone(), dm);

            for i in next_seq..lpb_seq {
                if !self.pending.contains_key(&MessageId(from.clone(), i)) {
                    try!(self.gossip(LpbLinkMessage::Request { 
                        from: self.id.clone(), 
                        msg_from: from.clone(), 
                        msg_seq: i, 
                        ttl: self.rounds - 1 
                    }));
                }
            }

            self.timers.as_mut().unwrap().push(try!(Self::set_timeout(self.delta, msg_id)));
        }

        Ok(())
    }

    fn handle_timeout(&mut self, msg_id: &MessageId) -> io::Result<()> {
        
        let mut next_seq = *self.next_seq.get(&msg_id.0).unwrap();

        while msg_id.1 >= next_seq {
            debug!("timeout waiting for {:?}. next_seq = {}", msg_id, next_seq);

            if ! try!(self.deliver_pending(&msg_id.0)) {
                debug!("skipped over missing message {} {}", msg_id.0, next_seq);
            }

            next_seq += 1;
            self.next_seq.insert(msg_id.0.clone(), next_seq);
        }

        Ok(())
    }

    fn set_timeout(interval: Duration, msg_id: MessageId) -> Result<Box<Future<Item=MessageId, Error=io::Error>>, io::Error> {
        let std_interval = try!(interval.to_std()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, "duration out of range")));

        Ok(Box::new(Timer::default()
            .sleep(std_interval)
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "timer error"))
            .map(|_| msg_id)
        ) as Box<Future<Item=MessageId, Error=io::Error>>)
    }

    fn handle_lpb_request(&mut self, from: String, msg_from: String, msg_seq: u64, ttl: u32) -> io::Result<()> {
        if let Some(dm) = self.stored.get(&MessageId(msg_from.clone(), msg_seq)) {
            debug!("have data to fulfil request for {}, {}", msg_from, msg_seq);
            try!(self.send(from, LpbLinkMessage::Data { 
                from: dm.from.clone(), 
                seq: dm.seq, 
                value: serde_json::to_value(dm.value.clone()), 
            }));
        } else if ttl > 0 {
            debug!("relaying request for {}, {}", msg_from, msg_seq);
            try!(self.gossip(LpbLinkMessage::Request { 
                from: from, 
                msg_from: msg_from, 
                msg_seq: msg_seq, 
                ttl: ttl - 1 
            }));
        }

        Ok(())
    }

    fn handle_lpb_data(&mut self, from: String, seq: u64, value: serde_json::Value) -> io::Result<()> {
        let msg_id = MessageId(from.clone(), seq);
        let typed_value = try!(serde_json::from_value(value)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, "json decoding error")));
        self.pending.insert(msg_id, Message { from: from.clone(), seq: seq, value: typed_value });

        while try!(self.deliver_pending(&from)) {
            // pass
        }

        Ok(())
    }

    fn deliver_pending(&mut self, node_id: &str) -> io::Result<bool> {
        if let Some(next_seq) = self.next_seq.get_mut(node_id) {
            let key = MessageId(node_id.to_owned(), *next_seq);
            if let Some(msg) = self.pending.remove(&key) {
                *next_seq += 1;
                self.to_deliver.push_back(msg);
                debug!("delivered pending {:?}", key);
                return Ok(true);
            }
        }

        Ok(false)
    }

    fn send(&self, to: NodeId, lm: LpbLinkMessage) -> io::Result<()> {
        let lm_value = serde_json::to_value(lm);

        let mm = multiplex::Message::new(Some(to), self.id.clone(), self.multiplex_key.clone(), lm_value);

        self.link.send(multiplex::Command::SendMessage { msg: mm })
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

    fn decode_link_message(&self, link_msg: multiplex::Message) -> io::Result<LpbLinkMessage> {
        let gm: LpbLinkMessage = try!(serde_json::from_value(link_msg.body)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, "json decoding error")));
        
        Ok(gm)
    }
}

impl <T> Stream for LazyProbabilisticBroadcast<T> where T: Serialize + DeserializeOwned + Clone + Send + 'static {
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

        while let Async::Ready(Some(upb::Message { from, seq: upb_seq, value: lpb_msg })) = try!(self.upb.poll()) {
            debug!("gossip received broadcast message from: {} {:?}", from, lpb_msg);

            match lpb_msg {
                LpbMessage::Data { seq: lpb_seq, value } => {
                    try!(self.handle_upb_data(from, upb_seq, lpb_seq, value));
                }
            }
        }

        while let Async::Ready(Some(link_msg)) = try!(self.link_rx.poll()) {
            debug!("gossip received link message: {:?}", link_msg);

            let gm = try!(self.decode_link_message(link_msg));
            
            match gm {
                LpbLinkMessage::Data { from, seq, value } => {
                    try!(self.handle_lpb_data(from, seq, value));
                },
                LpbLinkMessage::Request { from, msg_from, msg_seq, ttl } => {
                    try!(self.handle_lpb_request(from, msg_from, msg_seq, ttl));
                }
            }
        }

        let mut new_timers: Vec<Box<Future<Item=MessageId, Error=io::Error>>> = vec![];
        let mut old_timers = self.timers.take().unwrap();
        for mut t in old_timers.drain(0..) {
            if let Async::Ready(msg_id) = try!(t.poll()) {
                try!(self.handle_timeout(&msg_id));
            } else {
                new_timers.push(t);
            }
        }
        self.timers = Some(new_timers);

        Ok(match self.to_deliver.pop_front() {
            Some(v) => Async::Ready(Some(v)),
            None => Async::NotReady, 
        })
    }
}
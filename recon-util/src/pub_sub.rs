use std::collections::{BTreeMap,VecDeque};
use std::sync::{Arc,Mutex};
use std::result::Result;
use std::io;
use futures::{Async,AsyncSink};
use futures::future::Future;
use futures::sink::Sink;
use futures::stream::Stream;
use futures::sync::mpsc::{channel,Sender,Receiver};
use uuid::Uuid;

#[derive(Clone,Debug,PartialEq,Eq,PartialOrd,Ord)]
pub struct SubscriptionId {
    id: Uuid,
}

impl SubscriptionId {
    pub fn new() -> SubscriptionId { 
        SubscriptionId {
            id: Uuid::new_v4()
        }
    }
}

pub struct Subscription<T> where T: Clone {
    id: SubscriptionId,
    subscriptions: Arc<Mutex<Subscriptions<T>>>,
}

impl <T> Subscription<T> where T: Clone {
    pub fn new(id: SubscriptionId, subscriptions: Arc<Mutex<Subscriptions<T>>>) -> Subscription<T> {
        Subscription {
            id: id,
            subscriptions: subscriptions,
        }
    }
}

impl <T> Drop for Subscription<T> where T: Clone {
    fn drop(&mut self) {
        let mut subs = self.subscriptions.lock().expect("mutex unlock error");

        subs.unsubscribe(&self.id).expect("unsubscribe error");
    }
}

pub trait Subscribable {
    type Item: Clone;
    type Error;

    fn subscribe(&self, sender: Sender<Self::Item>) -> Result<Subscription<Self::Item>,Self::Error>;
    fn unsubscribe(&self, subscription: Subscription<Self::Item>) -> Result<(),Self::Error>;
}

pub struct Subscriptions<T> where T: Clone {
    outbound: Vec<(SubscriptionId,T)>,
    pending: Vec<SubscriptionId>,
    subscriptions: BTreeMap<SubscriptionId, Sender<T>>,
}

impl <T> Subscriptions<T> where T: Clone {
    pub fn new() -> Subscriptions<T> {
        Subscriptions {
            outbound: vec![],
            pending: vec![],
            subscriptions: BTreeMap::new(),
        }
    }

    pub fn subscribe(&mut self, sender: Sender<T>) -> Result<SubscriptionId,io::Error> {
        let subscription_id = SubscriptionId::new();
        debug!("subscribing {:?}", subscription_id);
        self.subscriptions.insert(subscription_id.clone(), sender);
        Ok(subscription_id)
    }

    pub fn unsubscribe(&mut self, subscription_id: &SubscriptionId) -> Result<Option<Sender<T>>,io::Error> {
        debug!("unsubsribing {:?}", subscription_id);
        Ok(self.subscriptions.remove(subscription_id))
    }
}

impl <T> Sink for Subscriptions<T> where T: Clone {
    type SinkItem = T;
    type SinkError = io::Error;

    fn start_send(&mut self, message: T) -> Result<AsyncSink<T>,Self::SinkError> {
        if !self.outbound.is_empty() {
            trace!("not ready to publish message");
            return Ok(AsyncSink::NotReady(message));
        }

        for (id,conn) in self.subscriptions.iter_mut() {
            match try!(conn.start_send(message.clone()).map_err(|e| io::Error::new(io::ErrorKind::Other, format!("{:?}",e)))) {
                AsyncSink::Ready => {
                    trace!("fan out message to {:?}", id);
                    self.pending.push(id.to_owned());
                },
                AsyncSink::NotReady(message) => {
                    trace!("not ready to fan out message to {:?}", id);
                    self.outbound.push((id.to_owned(), message));
                }
            }
        }
        
        Ok(AsyncSink::Ready)
    }

    fn poll_complete(&mut self) -> Result<Async<()>,Self::SinkError> {
        let mut new_pending = vec![];

        trace!("polling {} pending subscribers", self.pending.len());

        for p in self.pending.drain(..) {
            if let Some(mut conn) = self.subscriptions.get_mut(&p) {
                match try!(conn.poll_complete().map_err(|e| io::Error::new(io::ErrorKind::Other, format!("{:?}", e)))) {
                    Async::Ready(_) => {
                        trace!("subscriber {:?} completed", p);
                    },
                    Async::NotReady => {
                        trace!("subscriber {:?} not ready", p);
                        new_pending.push(p);
                    }
                }
            }
        }

        self.pending = new_pending;

        if self.pending.is_empty() {
            Ok(Async::Ready(()))
        } else {
            Ok(Async::NotReady)
        }
    }
}

pub struct PubSub<T> where T: Clone {
    complete: bool,
    outbound: VecDeque<T>,
    inflight: bool,
    subscriptions: Arc<Mutex<Subscriptions<T>>>,
    rx: Receiver<T>,
    tx: Sender<T>,
}

impl <T> PubSub<T> where T: Clone {
    pub fn new(buffering: usize) -> PubSub<T> {
        let (tx,rx) = channel(buffering);
        PubSub {
            complete: false,
            outbound: VecDeque::new(),
            inflight: false,
            subscriptions: Arc::new(Mutex::new(Subscriptions::new())),
            rx: rx,
            tx: tx,
        }
    }

    pub fn sender(&self) -> Sender<T> {
        return self.tx.clone();
    }
}

impl <T> Subscribable for PubSub<T> where T: Clone {
    type Item = T;
    type Error = io::Error;

    fn subscribe(&self, sender: Sender<T>) -> Result<Subscription<T>,Self::Error> {
        let mut subs = try!(self.subscriptions.lock().map_err(|_e| io::Error::new(io::ErrorKind::Other, "mutex lock error")));

        let id = try!(subs.subscribe(sender));

        Ok(Subscription::new(id, self.subscriptions.clone()))
    }

    fn unsubscribe(&self, subscription: Subscription<T>) -> Result<(),Self::Error> {
        drop(subscription);
        Ok(())
    }
}

impl <T> Future for PubSub<T> where T: Clone {
    type Item = ();
    type Error = io::Error;

    fn poll(&mut self) -> Result<Async<Self::Item>,Self::Error> {
        let mut subs = try!(self.subscriptions.lock().map_err(|_e| io::Error::new(io::ErrorKind::Other, "mutex lock error")));

        debug!("outbound messages: {}", self.outbound.len());

        if self.outbound.is_empty() {
            match try!(self.rx.poll().map_err(|e| io::Error::new(io::ErrorKind::Other, format!("error polling receiver: {:?}", e)))) {
                Async::Ready(Some(message)) => {
                    trace!("received message to publish");
                    self.outbound.push_back(message);
                },
                Async::Ready(None) => {
                    trace!("received end of stream");
                    self.complete = true;
                },
                Async::NotReady => {}
            }
        }

        if let Some(message) = self.outbound.pop_front() {
            match try!(subs.start_send(message)) {
                AsyncSink::Ready => {
                    trace!("started sending message");
                    self.inflight = true;
                },
                AsyncSink::NotReady(message) => {
                    trace!("not ready to send message");
                    self.outbound.push_front(message);
                }
            }
        }

        if self.inflight {
            match try!(subs.poll_complete()) {
                Async::Ready(_) => {
                    trace!("completed sending message");
                    self.inflight = false;
                },
                Async::NotReady => {}
            }
        }

        if self.complete {
            Ok(Async::Ready(()))
        } else {
            Ok(Async::NotReady)
        }
    }
}
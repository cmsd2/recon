use std::collections::{BTreeMap,VecDeque};
use std::sync::{Arc,Mutex};
use std;
use futures::{Async,AsyncSink};
use futures::future::Future;
use futures::sink::Sink;
use futures::stream::Stream;
use futures::sync::mpsc::{channel,Sender,Receiver};
use uuid::Uuid;
use ::errors::*;

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

pub struct Subscription<T> where T: Clone + Send + 'static {
    id: SubscriptionId,
    subscriptions: Arc<Mutex<Subscriptions<T>>>,
}

impl <T> Subscription<T> where T: Clone + Send + 'static {
    pub fn new(id: SubscriptionId, subscriptions: Arc<Mutex<Subscriptions<T>>>) -> Subscription<T> {
        Subscription {
            id: id,
            subscriptions: subscriptions,
        }
    }
}

impl <T> Drop for Subscription<T> where T: Clone + Send + 'static {
    fn drop(&mut self) {
        let mut subs = self.subscriptions.lock().expect("mutex unlock error");

        subs.unsubscribe(&self.id).expect("unsubscribe error");
    }
}

pub trait Subscribable {
    type Item: Clone + Send + 'static;
    type Error: std::error::Error + Send + 'static;

    fn subscribe(&self, sender: Sender<Self::Item>) -> std::result::Result<Subscription<Self::Item>,Self::Error>;
    fn unsubscribe(&self, subscription: Subscription<Self::Item>) -> std::result::Result<(),Self::Error>;
}

pub struct Subscriptions<T> where T: Clone + Send + 'static {
    outbound: Vec<(SubscriptionId,T)>,
    pending: Vec<SubscriptionId>,
    subscriptions: BTreeMap<SubscriptionId, Sender<T>>,
}

impl <T> Subscriptions<T> where T: Clone + Send + 'static {
    pub fn new() -> Subscriptions<T> {
        Subscriptions {
            outbound: vec![],
            pending: vec![],
            subscriptions: BTreeMap::new(),
        }
    }

    pub fn subscribe(&mut self, sender: Sender<T>) -> Result<SubscriptionId> {
        let subscription_id = SubscriptionId::new();
        debug!("subscribing {:?}", subscription_id);
        self.subscriptions.insert(subscription_id.clone(), sender);
        Ok(subscription_id)
    }

    pub fn unsubscribe(&mut self, subscription_id: &SubscriptionId) -> Result<Option<Sender<T>>> {
        debug!("unsubsribing {:?}", subscription_id);
        Ok(self.subscriptions.remove(subscription_id))
    }
}

impl <T> Sink for Subscriptions<T> where T: Clone + Send + 'static {
    type SinkItem = T;
    type SinkError = Error;

    fn start_send(&mut self, message: T) -> Result<AsyncSink<T>> {
        if !self.outbound.is_empty() {
            trace!("not ready to publish message");
            return Ok(AsyncSink::NotReady(message));
        }

        for (id,conn) in self.subscriptions.iter_mut() {
            match try!(conn.start_send(message.clone()).map_err(|e| Error::with_chain(e, "error sending message to subscriber") )) {
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

    fn poll_complete(&mut self) -> Result<Async<()>> {
        let mut new_pending = vec![];

        trace!("polling {} pending subscribers", self.pending.len());

        for p in self.pending.drain(..) {
            if let Some(mut conn) = self.subscriptions.get_mut(&p) {
                match try!(conn.poll_complete().map_err(|e| Error::with_chain(e, "error waiting for published messages to be delivered"))) {
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

pub struct PubSub<T> where T: Clone + Send + 'static {
    complete: bool,
    outbound: VecDeque<T>,
    inflight: bool,
    subscriptions: Arc<Mutex<Subscriptions<T>>>,
    rx: Receiver<T>,
    tx: Sender<T>,
}

impl <T> PubSub<T> where T: Clone + Send + 'static {
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

impl <T> Subscribable for PubSub<T> where T: Clone + Send + 'static {
    type Item = T;
    type Error = Error;

    fn subscribe(&self, sender: Sender<T>) -> Result<Subscription<T>> {
        let mut subs = try!(self.subscriptions.lock().map_err(|e| Error::from_kind(ErrorKind::LockError(format!("{}", e))) ));

        let id = try!(subs.subscribe(sender));

        Ok(Subscription::new(id, self.subscriptions.clone()))
    }

    fn unsubscribe(&self, subscription: Subscription<T>) -> Result<()> {
        drop(subscription);
        Ok(())
    }
}

impl <T> Future for PubSub<T> where T: Clone + Send + 'static {
    type Item = ();
    type Error = Error;

    fn poll(&mut self) -> Result<Async<Self::Item>> {
        let mut subs = try!(self.subscriptions.lock().map_err(|e| Error::from_kind(ErrorKind::LockError(format!("{}", e)))));

        debug!("outbound messages: {}", self.outbound.len());

        if self.outbound.is_empty() {
            match try!(self.rx.poll().map_err(|()| Error::from_kind(ErrorKind::PollError))) {
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
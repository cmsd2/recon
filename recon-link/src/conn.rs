use futures::{Async, AsyncSink, Future, Poll};
use futures::stream::Stream;
use futures::sink::Sink;
use state_machine_future::RentToOwn;
use tokio_core::reactor::Handle;
use tokio_retry::{self, Retry};
use tokio_retry::strategy::{FibonacciBackoff, jitter};
use std::io;
use std::error;
use std::time::{Duration, Instant};
use std::fmt;
use std::collections::VecDeque;

pub type ConnectionError = io::Error;
pub type SessionId = u32;

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Connected {
        session_id: SessionId,
    }
}

pub enum Message<Item> {
    Packet {
        session_id: SessionId,
        content: Item,
    },
    Control {
        event: Event,
    }
}

pub struct TimestampedItem<Item> {
    pub timestamp: Instant,
    pub item: Item,
}

impl <Item> TimestampedItem<Item> {
    pub fn new(item: Item) -> TimestampedItem<Item> {
        TimestampedItem {
            timestamp: Instant::now(),
            item: item,
        }
    }

    pub fn age(&self) -> Duration {
        Instant::now().duration_since(self.timestamp)
    }

    pub fn older_than(&self, age: Duration) -> bool {
        self.age().gt(&age)
    }
}

impl <Item> Clone for Message<Item> where Item: Clone {
    fn clone(&self) -> Message<Item> {
        match self {
            &Message::Packet { session_id, ref content } => {
                Message::Packet {
                    session_id: session_id,
                    content: content.clone()
                }
            },
            &Message::Control { ref event } => {
                Message::Control {
                    event: event.clone()
                }
            }
        }
    }
}

impl <Item> fmt::Debug for Message<Item> where Item: fmt::Debug {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            &Message::Packet { session_id, ref content } => {
                write!(f, "Message::Packet<session_id={:?}, content={:?}>", session_id, content)
            },
            &Message::Control { ref event } => {
                write!(f, "Message::Control<event={:?}>", event)
            }
        }
    }
}

pub trait NewTransport {
    type Future: Future<Item=Self::Transport,Error=Self::Error>;
    type Transport;
    type Error: error::Error + Sync + Send;

    fn new_transport(&self) -> Self::Future;
}

#[derive(StateMachineFuture)]
pub enum Connection<Item, S, K, T, N> where S: Stream<Item=Item,Error=io::Error>, K: Sink<SinkItem=Message<Item>,SinkError=io::Error>, T: Stream<Item=Item,Error=io::Error>+Sink<SinkItem=Item,SinkError=io::Error>, N: NewTransport<Transport=T>+Clone+'static {
    #[state_machine_future(start, transitions(Connecting, Finished, Error))]
    NotConnected {
        handle: Handle,
        stream: S,
        sink: K,
        error_count: u32,
        session_id: u32,
        new_transport: N,
        outbound_max: usize,
        outbound_max_age: Duration,
    },

    #[state_machine_future(transitions(NotConnected, Connected, Finished, Error))]
    Connecting {
        handle: Handle,
        stream: S,
        sink: K,
        tcp_future: Box<Future<Item=T,Error=io::Error>>,
        error_count: u32,
        session_id: u32,
        new_transport: N,
        outbound_max: usize,
        outbound_max_age: Duration,
    },

    #[state_machine_future(transitions(Connected, NotConnected, Finished, Error))]
    Connected {
        handle: Handle,
        stream: S,
        sink: K,
        tcp: T,
        outbound: VecDeque<TimestampedItem<Item>>,
        outbound_max: usize,
        outbound_max_age: Duration,
        outbound_inflight: u32,
        inbound: VecDeque<Message<Item>>,
        inbound_inflight: u32,
        error_count: u32,
        session_id: u32,
        new_transport: N,
    },

    #[state_machine_future(ready)]
    Finished(()),

    #[state_machine_future(error)]
    Error(ConnectionError),
}

impl <Item, S, K, T, N> Connection<Item, S, K, T, N> where S: Stream<Item=Item,Error=io::Error>, K: Sink<SinkItem=Message<Item>,SinkError=io::Error>, T: Stream<Item=Item,Error=io::Error>+Sink<SinkItem=Item,SinkError=io::Error>, N: NewTransport<Transport=T>+Clone+'static {
    pub fn new(handle: Handle, stream: S, sink: K, new_transport: N, outbound_max: usize, outbound_max_age: Duration) -> ConnectionFuture<Item, S, K, T, N> {
        debug!("constructing new connection future");
        Connection::start(handle, stream, sink, 0, 0, new_transport, outbound_max, outbound_max_age)
    }
}

impl <Item, S, K, T, N> PollConnection<Item, S, K, T, N> for Connection<Item, S, K, T, N> where S: Stream<Item=Item,Error=io::Error>, K: Sink<SinkItem=Message<Item>,SinkError=io::Error>, T: Stream<Item=Item,Error=io::Error>+Sink<SinkItem=Item,SinkError=io::Error>, N: NewTransport<Transport=T>+Clone+'static {
    fn poll_not_connected<'a>(not_connected: &'a mut RentToOwn<'a, NotConnected<Item, S, K, T, N>>) -> Poll<AfterNotConnected<Item, S, K, T, N>, ConnectionError> {
        trace!("not connected");
        
        let retry_strategy = FibonacciBackoff ::from_millis(1)
            .max_delay(Duration::from_millis(2000))
            .map(jitter);
        
        let handle = not_connected.handle.clone();
        let new_transport = not_connected.new_transport.clone();

        let tcp_future = Box::new(Retry::spawn(handle, retry_strategy, move || {
            new_transport.new_transport()
        }).map_err(|e| {
            match e {
                tokio_retry::Error::OperationError(e) => io::Error::new(io::ErrorKind::Other, e),
                tokio_retry::Error::TimerError(e) => e,
            }
        })) as (Box<Future<Item=T,Error=io::Error>>);

        let not_connected = not_connected.take();
        Ok(Async::Ready(Connecting {
            handle: not_connected.handle,
            stream: not_connected.stream,
            sink: not_connected.sink,
            tcp_future: tcp_future,
            error_count: not_connected.error_count,
            session_id: not_connected.session_id,
            new_transport: not_connected.new_transport,
            outbound_max: not_connected.outbound_max,
            outbound_max_age: not_connected.outbound_max_age,
        }.into()))
    }

    fn poll_connecting<'a>(connecting: &'a mut RentToOwn<'a, Connecting<Item, S, K, T, N>>) -> Poll<AfterConnecting<Item, S, K, T, N>, ConnectionError> {
        trace!("connecting");

        let tcp = try_ready!(connecting.tcp_future.poll());

        let connecting = connecting.take();
        Ok(Async::Ready(Connected {
            handle: connecting.handle,
            stream: connecting.stream,
            sink: connecting.sink,
            tcp: tcp,
            inbound: VecDeque::from(vec![
                Message::Control {
                    event: Event::Connected {
                        session_id: connecting.session_id
                    }
                }
            ]),
            inbound_inflight: 0,
            outbound: VecDeque::new(),
            outbound_inflight: 0,
            outbound_max: connecting.outbound_max,
            outbound_max_age: connecting.outbound_max_age,
            error_count: connecting.error_count,
            session_id: connecting.session_id,
            new_transport: connecting.new_transport,
        }.into()))
    }

    fn poll_connected<'a>(connected: &'a mut RentToOwn<'a, Connected<Item, S, K, T, N>>) -> Poll<AfterConnected<Item, S, K, T, N>, ConnectionError> {
        trace!("connected");

        // we make progress if any poll returns Ready
        // if we make progress then we return a state or state change
        // otherwise return NotReady
        let mut progress = false;
        
        let mut received = connected.inbound.pop_front();
        if received.is_none() && connected.inbound_inflight == 0 {
            received = match connected.tcp.poll() {
                Ok(Async::Ready(Some(msg))) => {
                    trace!("transport received msg");
                    progress = true;
                    Some(Message::Packet{session_id: connected.session_id, content: msg})
                },
                Ok(Async::Ready(None)) => {
                    trace!("transport returned end of stream");

                    let connected = connected.take();

                    return Ok(Async::Ready(NotConnected {
                        handle: connected.handle,
                        stream: connected.stream,
                        sink: connected.sink,
                        error_count: connected.error_count,
                        session_id: connected.session_id + 1,
                        new_transport: connected.new_transport,
                        outbound_max: connected.outbound_max,
                        outbound_max_age: connected.outbound_max_age,
                    }.into()));
                },
                Ok(Async::NotReady) => {
                    trace!("transport not ready to read");
                    None
                },
                Err(e) => {
                    trace!("transport read error: {:?}", e);
                    let connected = connected.take();

                    return Ok(Async::Ready(NotConnected {
                        handle: connected.handle,
                        stream: connected.stream,
                        sink: connected.sink,
                        error_count: connected.error_count + 1,
                        session_id: connected.session_id + 1,
                        new_transport: connected.new_transport,
                        outbound_max: connected.outbound_max,
                        outbound_max_age: connected.outbound_max_age,
                    }.into()));
                }
            };
        }

        while connected.outbound.len() < connected.outbound_max {
            match try!(connected.stream.poll()) {
                Async::Ready(Some(msg)) => {
                    progress = true;
                    connected.outbound.push_back(TimestampedItem::new(msg));
                },
                Async::Ready(None) => {
                    trace!("stream finished");
                    return Ok(Async::Ready(Finished(()).into()));
                },
                Async::NotReady => {
                    trace!("stream not ready to read");
                    break;
                }
            };
        }

        if let Some(TimestampedItem{ timestamp, item: sending }) = connected.outbound.pop_front() {
            match connected.tcp.start_send(sending) {
                Ok(AsyncSink::Ready) => {
                    trace!("transport wrote message");
                    progress = true;
                    connected.outbound_inflight += 1;
                },
                Ok(AsyncSink::NotReady(sending)) => {
                    trace!("tcpstream not ready to write");

                    let timestamped_msg = TimestampedItem { timestamp: timestamp, item: sending };
                    if timestamped_msg.older_than(connected.outbound_max_age) {
                        trace!("message older than max age {:?}. reconnecting", timestamped_msg.age());
                        
                        let connected = connected.take();

                        return Ok(Async::Ready(NotConnected {
                            handle: connected.handle,
                            stream: connected.stream,
                            sink: connected.sink,
                            error_count: connected.error_count,
                            session_id: connected.session_id + 1,
                            new_transport: connected.new_transport,
                            outbound_max: connected.outbound_max,
                            outbound_max_age: connected.outbound_max_age,
                        }.into()));
                    } else {
                        connected.outbound.push_front(timestamped_msg);
                    }
                },
                Err(err) => {
                    trace!("transport write error: {:?}", err);                    
                    let connected = connected.take();

                    return Ok(Async::Ready(NotConnected {
                        handle: connected.handle,
                        stream: connected.stream,
                        sink: connected.sink,
                        error_count: connected.error_count + 1,
                        session_id: connected.session_id + 1,
                        new_transport: connected.new_transport,
                        outbound_max: connected.outbound_max,
                        outbound_max_age: connected.outbound_max_age,
                    }.into()));
                }
            }
        }

        if let Some(msg) = received {
            match try!(connected.sink.start_send(msg)) {
                AsyncSink::Ready => {
                    trace!("sink started send");
                    progress = true;
                    connected.inbound_inflight += 1;
                },
                AsyncSink::NotReady(msg) => {
                    trace!("sink not ready");
                    connected.inbound.insert(0, msg);
                },
            }
        }

        if connected.inbound_inflight != 0 {
            match try!(connected.sink.poll_complete()) {
                Async::Ready(()) => {
                    trace!("sink polled complete");
                    progress = true;
                    connected.inbound_inflight = 0;
                },
                Async::NotReady => {
                    trace!("sink polled not ready");
                    // do nothing
                }
            }
        }

        if connected.outbound_inflight != 0 {
            match try!(connected.tcp.poll_complete()) {
                Async::Ready(()) => {
                    trace!("transport poll complete");
                    progress = true;
                    connected.outbound_inflight = 0;
                },
                Async::NotReady => {
                    trace!("transport sink polled not ready");
                    // do nothing
                }
            }
        }

        if progress {
            trace!("made progress");

            let connected = connected.take();

            return Ok(Async::Ready(Connected {
                handle: connected.handle,
                stream: connected.stream,
                sink: connected.sink,
                tcp: connected.tcp,
                inbound: connected.inbound,
                inbound_inflight: connected.inbound_inflight,
                outbound: connected.outbound,
                outbound_max: connected.outbound_max,
                outbound_max_age: connected.outbound_max_age,
                outbound_inflight: connected.outbound_inflight,
                error_count: connected.error_count,
                session_id: connected.session_id,
                new_transport: connected.new_transport,
            }.into()));
        } else {
            trace!("did not make progress");
            
            return Ok(Async::NotReady);
        }
    }
}

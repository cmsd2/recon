use futures::{Async, AsyncSink, Future, Poll};
use futures::stream::Stream;
use futures::sink::Sink;
use state_machine_future::RentToOwn;
use tokio_core::reactor::Handle;
use tokio_core::net::TcpStream;
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_retry::{self, Retry};
use tokio_retry::strategy::{FibonacciBackoff, jitter};
use std::ops::Deref;
use std::net::SocketAddr;
use std::io;
use std::time::Duration;
use bytes::{Bytes, BytesMut, Buf, IntoBuf};

pub type ConnectionError = io::Error;

#[derive(Clone, PartialEq, Debug)]
pub struct Message {
    pub session_id: u32,
    pub content: Vec<u8>,
}

impl Message {
    pub fn new(session_id: u32) -> Message {
        Message {
            session_id: session_id,
            content: vec![],
        }
    }

    pub fn from_bytes(session_id: u32, content: Vec<u8>) -> Message {
        Message {
            session_id: session_id,
            content: content,
        }
    }
}

#[derive(StateMachineFuture)]
pub enum Connection<Item: Into<Vec<u8>>, S: Stream<Item=Item,Error=io::Error>, K: Sink<SinkItem=Message,SinkError=io::Error>> {
    #[state_machine_future(start, transitions(Connecting, Finished, Error))]
    NotConnected {
        addr: SocketAddr,
        handle: Handle,
        stream: S,
        sink: K,
        error_count: u32,
        session_id: u32,
    },

    #[state_machine_future(transitions(NotConnected, Connected, Finished, Error))]
    Connecting {
        addr: SocketAddr,
        handle: Handle,
        stream: S,
        sink: K,
        tcp_future: Box<Future<Item=TcpStream,Error=io::Error>>,
        error_count: u32,
        session_id: u32,
    },

    #[state_machine_future(transitions(Connected, NotConnected, Finished, Error))]
    Connected {
        addr: SocketAddr,
        handle: Handle,
        stream: S,
        sink: K,
        tcp: TcpStream,
        outbound: Option<Bytes>,
        inbound: Option<Message>,
        inbound_inflight: u32,
        error_count: u32,
        session_id: u32,
    },

    #[state_machine_future(ready)]
    Finished(()),

    #[state_machine_future(error)]
    Error(ConnectionError),
}

impl <Item, S, K> Connection<Item, S, K> where Item: Into<Vec<u8>>, S: Stream<Item=Item,Error=io::Error>, K: Sink<SinkItem=Message,SinkError=io::Error> {
    pub fn new(addr: SocketAddr, handle: Handle, stream: S, sink: K) -> ConnectionFuture<Item, S, K> {
        debug!("constructing new connection future");
        Connection::start(addr, handle, stream, sink, 0, 0)
    }
}

impl <Item, S, K> PollConnection<Item, S, K> for Connection<Item, S, K> where Item: Into<Vec<u8>>, S: Stream<Item=Item,Error=io::Error>, K: Sink<SinkItem=Message,SinkError=io::Error> {
    fn poll_not_connected<'a>(not_connected: &'a mut RentToOwn<'a, NotConnected<Item, S, K>>) -> Poll<AfterNotConnected<Item, S, K>, ConnectionError> {
        trace!("not connected to {}", not_connected.addr);
        
        let retry_strategy = FibonacciBackoff ::from_millis(1)
            .max_delay(Duration::from_millis(2000))
            .map(jitter);
        
        let addr = not_connected.addr.clone();
        let handle = not_connected.handle.clone();

        let tcp_future = Box::new(Retry::spawn(not_connected.handle.clone(), retry_strategy, move || {
            TcpStream::connect(&addr, &handle)
        }).map_err(|e| {
            match e {
                tokio_retry::Error::OperationError(e) => e,
                tokio_retry::Error::TimerError(e) => e,
            }
        })) as Box<Future<Item=TcpStream,Error=io::Error>>;

        let not_connected = not_connected.take();
        Ok(Async::Ready(Connecting {
            addr: not_connected.addr,
            handle: not_connected.handle,
            stream: not_connected.stream,
            sink: not_connected.sink,
            tcp_future: tcp_future,
            error_count: not_connected.error_count,
            session_id: not_connected.session_id,
        }.into()))
    }

    fn poll_connecting<'a>(connecting: &'a mut RentToOwn<'a, Connecting<Item, S, K>>) -> Poll<AfterConnecting<Item, S, K>, ConnectionError> {
        trace!("connecting to {}", connecting.addr);

        let tcp = try_ready!(connecting.tcp_future.poll());

        let connecting = connecting.take();
        Ok(Async::Ready(Connected {
            addr: connecting.addr,
            handle: connecting.handle,
            stream: connecting.stream,
            sink: connecting.sink,
            tcp: tcp,
            inbound: None,
            inbound_inflight: 0,
            outbound: None,
            error_count: connecting.error_count,
            session_id: connecting.session_id,
        }.into()))
    }

    fn poll_connected<'a>(connected: &'a mut RentToOwn<'a, Connected<Item, S, K>>) -> Poll<AfterConnected<Item, S, K>, ConnectionError> {
        trace!("connected to {}", connected.addr);

        // we make progress if any poll returns Ready
        // if we make progress then we return a state or state change
        // otherwise return NotReady
        let mut progress = false;
        
        let mut received = connected.inbound.take();
        if received.is_none() && connected.inbound_inflight == 0 {
            let mut receiving_buf = BytesMut::new();
            received = match connected.tcp.read_buf(&mut receiving_buf) {
                Ok(Async::Ready(len)) => {
                    trace!("tcpstream received {} bytes", len);
                    progress = true;
                    Some(Message::from_bytes(connected.session_id, receiving_buf.freeze().to_vec()))
                },
                Ok(Async::NotReady) => {
                    trace!("tcpstream not ready to read");
                    None
                },
                Err(e) => {
                    trace!("tcpstream read error: {:?}", e);
                    let connected = connected.take();

                    return Ok(Async::Ready(NotConnected {
                        addr: connected.addr,
                        handle: connected.handle,
                        stream: connected.stream,
                        sink: connected.sink,
                        error_count: connected.error_count + 1,
                        session_id: connected.session_id + 1,
                    }.into()));
                }
            };
        }

        let mut sending_buf = connected.outbound.take();
        if sending_buf.is_none() {
            sending_buf = match try!(connected.stream.poll()) {
                Async::Ready(Some(msg)) => {
                    progress = true;
                    let mut buf = Some(Bytes::new());
                    let msg_vec: Vec<u8> = msg.into();
                    trace!("stream received new message of length {}", msg_vec.len());
                    set_bytes(&mut buf, msg_vec);

                    buf
                },
                Async::Ready(None) => {
                    trace!("stream finished");
                    return Ok(Async::Ready(Finished(()).into()));
                },
                Async::NotReady => {
                    trace!("stream not ready to read");
                    None
                }
            };
        }

        if let Some(buf) = sending_buf {
            let total_len = buf.len();
            let mut buf = buf.into_buf();
            match connected.tcp.write_buf(&mut buf) {
                Ok(Async::Ready(len)) => {
                    trace!("tcpstream wrote {} of {} bytes", len, total_len);
                    progress = true;
                    if buf.has_remaining() {
                        set_bytes(&mut connected.outbound, buf.bytes());
                    }
                },
                Ok(Async::NotReady) => {
                    trace!("tcpstream not ready to write");
                    set_bytes(&mut connected.outbound, buf.bytes());
                },
                Err(err) => {
                    trace!("tcpstream write error: {:?}", err);
                    set_bytes(&mut connected.outbound, buf.bytes());
                    
                    let connected = connected.take();

                    return Ok(Async::Ready(NotConnected {
                        addr: connected.addr,
                        handle: connected.handle,
                        stream: connected.stream,
                        sink: connected.sink,
                        error_count: connected.error_count + 1,
                        session_id: connected.session_id + 1,
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
                    connected.inbound = Some(msg);
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

        if progress {
            trace!("made progress");

            let connected = connected.take();

            return Ok(Async::Ready(Connected {
                addr: connected.addr,
                handle: connected.handle,
                stream: connected.stream,
                sink: connected.sink,
                tcp: connected.tcp,
                inbound: connected.inbound,
                inbound_inflight: connected.inbound_inflight,
                outbound: connected.outbound,
                error_count: connected.error_count,
                session_id: connected.session_id,
            }.into()));
        } else {
            trace!("did not make progress");
            
            return Ok(Async::NotReady);
        }
    }
}

fn set_bytes<'a, B>(maybe_buffer: &mut Option<Bytes>, content: B) where B: Deref<Target=[u8]> {
     let mut buf_remaining = Bytes::new();
    buf_remaining.extend_from_slice(&content);
    *maybe_buffer = Some(buf_remaining);
}
use futures::{Async, AsyncSink, Future, Poll};
use futures::stream::Stream;
use futures::sink::Sink;
use state_machine_future::RentToOwn;
use tokio_core::reactor::Handle;
use tokio_core::net::TcpStream;
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_io::io::read;
use tokio_retry::{self, Retry};
use tokio_retry::strategy::{FibonacciBackoff, jitter};
use std::net::SocketAddr;
use std::io;
use std::time::Duration;
use bytes::{Bytes, BytesMut, Buf, IntoBuf};

pub type ConnectionError = io::Error;

#[derive(StateMachineFuture)]
pub enum Connection<Item: Into<Vec<u8>>, S: Stream<Item=Item,Error=io::Error>, K: Sink<SinkItem=Vec<u8>,SinkError=io::Error>> {
    #[state_machine_future(start, transitions(Connecting, Finished, Error))]
    NotConnected {
        addr: SocketAddr,
        handle: Handle,
        stream: S,
        sink: K,
        error_count: u32,
    },

    #[state_machine_future(transitions(NotConnected, Connected, Finished, Error))]
    Connecting {
        addr: SocketAddr,
        handle: Handle,
        stream: S,
        sink: K,
        tcp_future: Box<Future<Item=TcpStream,Error=io::Error>>,
    },

    #[state_machine_future(transitions(Connected, NotConnected, Finished, Error))]
    Connected {
        addr: SocketAddr,
        handle: Handle,
        stream: S,
        sink: K,
        tcp: TcpStream,
        outbound: Option<Bytes>,
        inbound: Option<Bytes>,
        inbound_inflight: u32,
    },

    #[state_machine_future(ready)]
    Finished(()),

    #[state_machine_future(error)]
    Error(ConnectionError),
}

impl <Item, S, K> Connection<Item, S, K> where Item: Into<Vec<u8>>, S: Stream<Item=Item,Error=io::Error>, K: Sink<SinkItem=Vec<u8>,SinkError=io::Error> {
    pub fn new(addr: SocketAddr, handle: Handle, stream: S, sink: K) -> ConnectionFuture<Item, S, K> {
        debug!("constructing new connection future");
        Connection::start(addr, handle, stream, sink, 0)
    }
}

impl <Item, S, K> PollConnection<Item, S, K> for Connection<Item, S, K> where Item: Into<Vec<u8>>, S: Stream<Item=Item,Error=io::Error>, K: Sink<SinkItem=Vec<u8>,SinkError=io::Error> {
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
        }.into()))
    }

    fn poll_connected<'a>(connected: &'a mut RentToOwn<'a, Connected<Item, S, K>>) -> Poll<AfterConnected<Item, S, K>, ConnectionError> {
        trace!("connected to {}", connected.addr);
        // use std::io::Read;

        // let buf = try_ready!(
        //         connected
        //             .stream
        //             .by_ref()
        //             .take(1)
        //             .map(|msg| msg.into())
        //             .concat2()
        //             .select(
        //                 read(connected.tcp.by_ref(), vec![])
        //                 .map(|(tcp, buf, len)| {
        //                     buf
        //                 })
        //             )
        //             .poll()
        //         );

        let mut progress = false;
        
        let mut received = connected.inbound.take();
        if received.is_none() && connected.inbound_inflight == 0 {
            let mut receiving_buf = BytesMut::new();
            received = match connected.tcp.read_buf(&mut receiving_buf) {
                Ok(Async::Ready(len)) => {
                    trace!("tcpstream received {} bytes", len);
                    progress = true;
                    Some(receiving_buf.freeze())
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
                        error_count: 0,
                    }.into()));
                }
            };
        }

        let mut sending_buf = connected.outbound.take();
        if sending_buf.is_none() {
            sending_buf = match try!(connected.stream.poll()) {
                Async::Ready(Some(msg)) => {
                    progress = true;
                    let mut buf = Bytes::new();
                    let msg_vec: Vec<u8> = msg.into();
                    trace!("stream received new message of length {}", msg_vec.len());
                    buf.extend_from_slice(&msg_vec[..]);

                    Some(buf)
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
                        let mut buf_remaining = Bytes::new();
                        buf_remaining.extend_from_slice(buf.bytes());
                        connected.outbound = Some(buf_remaining);
                    }
                },
                Ok(Async::NotReady) => {
                    trace!("tcpstream not ready to write");
                    let mut buf_remaining = Bytes::new();
                    buf_remaining.extend_from_slice(buf.bytes());
                    connected.outbound = Some(buf_remaining);
                },
                Err(err) => {
                    trace!("tcpstream write error: {:?}", err);
                    unimplemented!();
                }
            }
        }

        if let Some(buf) = received {
            match try!(connected.sink.start_send(buf.to_vec())) {
                AsyncSink::Ready => {
                    trace!("sink started send");
                    progress = true;
                    connected.inbound_inflight += 1;
                },
                AsyncSink::NotReady(buf) => {
                    trace!("sink not ready");
                    let mut buf_remaining = Bytes::new();
                    buf_remaining.extend_from_slice(&buf[..]);
                    connected.inbound = Some(buf_remaining);
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
                    //
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
            }.into()));
        } else {
            trace!("did not make progress");
            
            return Ok(Async::NotReady);
        }
    }

    // fn poll_connected_sending<'a>(connected: &'a mut RentToOwn<'a, ConnectedSending<Item, S>>) -> Poll<AfterConnectedSending<Item, S>, ConnectionError> {
    //     let mut buf = connected.buf.take().unwrap().into_buf();

    //     trace!("trying to write {} bytes", buf.remaining());
    //     match connected.tcp.write_buf(&mut buf) {
    //         Ok(Async::Ready(len)) => {
    //             trace!("wrote {} bytes", len);

    //             let mut connected = connected.take();

    //             if buf.has_remaining() {
    //                 let mut buf_remaining = Bytes::new();
    //                 buf_remaining.extend_from_slice(buf.bytes());
    //                 connected.buf = Some(buf_remaining);

    //                 return Ok(Async::Ready(ConnectedSending {
    //                     addr: connected.addr,
    //                     handle: connected.handle,
    //                     stream: connected.stream,
    //                     tcp: connected.tcp,
    //                     buf: connected.buf,
    //                 }.into()));
    //             } else if (connected.inbound.is_some()) {
    //                 return Ok(Async::Ready(ConnectedReceiving {
    //                     addr: connected.addr,
    //                     handle: connected.handle,
    //                     stream: connected.stream,
    //                     tcp: connected.tcp,
    //                 }.into()));
    //             } else {
    //                 return Ok(Async::Ready(Connected {
    //                     addr: connected.addr,
    //                     handle: connected.handle,
    //                     stream: connected.stream,
    //                     tcp: connected.tcp,
    //                 }.into()));
    //             }
    //         },
    //         Ok(Async::NotReady) => {
    //             trace!("not ready");
    //             let mut buf_remaining = Bytes::new();
    //             buf_remaining.extend_from_slice(buf.bytes());
    //             connected.buf = Some(buf_remaining);
    //             return Ok(Async::NotReady);
    //         },
    //         Err(err) => {
    //             debug!("error writing to socket {:?}", err);
    //             let connected = connected.take();

    //             return Ok(Async::Ready(NotConnected {
    //                 addr: connected.addr,
    //                 handle: connected.handle,
    //                 stream: connected.stream,
    //                 error_count: 0,
    //             }.into()));
    //         }
    //     }
    // }

    // fn poll_connected_receiving<'a>(connected: &'a mut RentToOwn<'a, ConnectedReceiving<Item, S>>) -> Poll<AfterConnectedReceiving<Item, S>, ConnectionError> {

    // }
}

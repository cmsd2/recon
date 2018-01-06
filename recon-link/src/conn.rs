use futures::{Async, Future, Poll};
use futures::stream::Stream;
use state_machine_future::RentToOwn;
use tokio_core::reactor::Handle;
use tokio_core::net::TcpStream;
use tokio_io::AsyncWrite;
use tokio_retry::{self, Retry};
use tokio_retry::strategy::{FibonacciBackoff, jitter};
use std::net::SocketAddr;
use std::io;
use std::time::Duration;
use bytes::{Bytes, Buf, IntoBuf};

pub type ConnectionError = io::Error;

#[derive(StateMachineFuture)]
pub enum Connection<Item: Into<Vec<u8>>, S: Stream<Item=Item,Error=io::Error>> {
    #[state_machine_future(start, transitions(Connecting, Finished, Error))]
    NotConnected {
        addr: SocketAddr,
        handle: Handle,
        stream: S,
        error_count: u32,
    },

    #[state_machine_future(transitions(NotConnected, Connected, Finished, Error))]
    Connecting {
        addr: SocketAddr,
        handle: Handle,
        stream: S,
        tcp_future: Box<Future<Item=TcpStream,Error=io::Error>>,
    },

    #[state_machine_future(transitions(ConnectedSending, NotConnected, Finished, Error))]
    Connected {
        addr: SocketAddr,
        handle: Handle,
        stream: S,
        tcp: TcpStream,
    },

    #[state_machine_future(transitions(Connected, ConnectedSending, NotConnected, Finished, Error))]
    ConnectedSending {
        addr: SocketAddr,
        handle: Handle,
        stream: S,
        tcp: TcpStream,
        buf: Option<Bytes>,
    },

    #[state_machine_future(ready)]
    Finished(()),

    #[state_machine_future(error)]
    Error(ConnectionError),
}

impl <Item, S> Connection<Item, S> where Item: Into<Vec<u8>>, S: Stream<Item=Item,Error=io::Error> {
    pub fn new(addr: SocketAddr, handle: Handle, stream: S) -> ConnectionFuture<Item, S> {
        debug!("constructing new connection future");
        Connection::start(addr, handle, stream, 0)
    }
}

impl <Item, S> PollConnection<Item, S> for Connection<Item, S> where Item: Into<Vec<u8>>, S: Stream<Item=Item,Error=io::Error> {
    fn poll_not_connected<'a>(not_connected: &'a mut RentToOwn<'a, NotConnected<Item, S>>) -> Poll<AfterNotConnected<Item, S>, ConnectionError> {
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
            tcp_future: tcp_future,
        }.into()))
    }

    fn poll_connecting<'a>(connecting: &'a mut RentToOwn<'a, Connecting<Item, S>>) -> Poll<AfterConnecting<Item, S>, ConnectionError> {
        trace!("connecting to {}", connecting.addr);

        let tcp = try_ready!(connecting.tcp_future.poll());

        let connecting = connecting.take();
        Ok(Async::Ready(Connected {
            addr: connecting.addr,
            handle: connecting.handle,
            stream: connecting.stream,
            tcp: tcp,
        }.into()))
    }

    fn poll_connected<'a>(connected: &'a mut RentToOwn<'a, Connected<Item, S>>) -> Poll<AfterConnected<Item, S>, ConnectionError> {
        trace!("connected to {}", connected.addr);

        let buf = try_ready!(connected.stream.poll()).map(|msg| {
            let mut buf = Bytes::new();
            let msg_vec: Vec<u8> = msg.into();
            trace!("found new message of length {}", msg_vec.len());
            buf.extend_from_slice(&msg_vec[..]);
            buf
        });

        let connected = connected.take();

        if let Some(buf) = buf {
            return Ok(Async::Ready(ConnectedSending {
                addr: connected.addr,
                handle: connected.handle,
                stream: connected.stream,
                tcp: connected.tcp,
                buf: Some(buf),
            }.into()))
        } else {
            return Ok(Async::Ready(Finished(()).into()));
        }
    }

    fn poll_connected_sending<'a>(connected: &'a mut RentToOwn<'a, ConnectedSending<Item, S>>) -> Poll<AfterConnectedSending<Item, S>, ConnectionError> {
        let mut buf = connected.buf.take().unwrap().into_buf();

        trace!("trying to write {} bytes", buf.remaining());
        match connected.tcp.write_buf(&mut buf) {
            Ok(Async::Ready(len)) => {
                trace!("wrote {} bytes", len);

                let mut connected = connected.take();

                if buf.has_remaining() {
                    let mut buf_remaining = Bytes::new();
                    buf_remaining.extend_from_slice(buf.bytes());
                    connected.buf = Some(buf_remaining);

                    return Ok(Async::Ready(ConnectedSending {
                        addr: connected.addr,
                        handle: connected.handle,
                        stream: connected.stream,
                        tcp: connected.tcp,
                        buf: connected.buf,
                    }.into()));
                } else {
                    return Ok(Async::Ready(Connected {
                        addr: connected.addr,
                        handle: connected.handle,
                        stream: connected.stream,
                        tcp: connected.tcp,
                    }.into()));
                }
            },
            Ok(Async::NotReady) => {
                trace!("not ready");
                let mut buf_remaining = Bytes::new();
                buf_remaining.extend_from_slice(buf.bytes());
                connected.buf = Some(buf_remaining);
                return Ok(Async::NotReady);
            },
            Err(err) => {
                debug!("error writing to socket {:?}", err);
                let connected = connected.take();

                return Ok(Async::Ready(NotConnected {
                    addr: connected.addr,
                    handle: connected.handle,
                    stream: connected.stream,
                    error_count: 0,
                }.into()));
            }
        }
    }

}

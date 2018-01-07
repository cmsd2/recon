use std::cell::RefCell;
use std::time::Duration;
use std::net::{self, SocketAddr};
use std::io;
use std::io::{Write};
use std::collections::VecDeque;
use futures::{self, Future, Poll, Async, Stream};
use futures::sync::mpsc::{channel, Sender, Receiver};
use tokio_core::reactor::{Core, Handle};
use tokio_core::net::{TcpStream};
use tokio_io::io::{copy, write_all};
use tokio_service::Service;
use tokio_timer::Timer;

use super::client::{NewClient, Client, LineClient};
use super::framing::{ReconFrame, FramedLineTransport, new_line_transport};
use super::service::{AsyncService};

pub enum ConnStream<S> where S: AsyncService<Request=String, Error=io::Error> {
    Connected(S),
    Connecting(Box<Future<Item=S,Error=io::Error>>),
    NotConnected
}

pub struct Conn<S> where S: AsyncService<Request=String, Error=io::Error> + 'static {
    handle: Handle,
    addr: SocketAddr,
    stream: Option<ConnStream<S>>,
    channel_tx: Sender<String>,
    channel_rx: Receiver<String>,
    factory: Box<NewClient<Request=String,Error=io::Error,Item=S,Io=TcpStream>>,
}

impl <S> Conn<S> where S: AsyncService<Request=String, Error=io::Error> + 'static {
    pub fn new(handle: Handle, addr: SocketAddr, factory: Box<NewClient<Request=String,Error=io::Error,Item=S,Io=TcpStream>>) -> io::Result<Conn<S>> {
        let (tx, rx) = try!(channel(&handle));

        Ok(Conn {
            handle: handle,
            addr: addr,
            stream: Some(ConnStream::NotConnected),
            channel_tx: tx,
            channel_rx: rx,
            factory: factory,
        })
    }

    pub fn channel(&self) -> Sender<String> {
        self.channel_tx.clone()
    }

    pub fn send<M>(&self, msg: M) where M: Into<String> {
        self.channel_tx.send(msg.into());
    }

    fn write_to_stream(&mut self, stream: &mut S) -> io::Result<()> {
        while let Async::Ready(Some(m)) = try!(self.channel_rx.poll()) {
            try!(stream.send(m));
        }

        try!(stream.poll());

        Ok(())
    }

    fn poll_stream(&mut self, stream: ConnStream<S>) -> io::Result<ConnStream<S>> {
        Ok(match stream {
            ConnStream::NotConnected => {
                trace!("not connected to {}", self.addr);

                let delay = Timer::default().sleep(Duration::from_secs(1))
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, "timer error"));
                
                let addr = self.addr.clone();
                let handle = self.handle.clone();

                /*let mut f = Box::new(delay.and_then(move |_| {
                    TcpStream::connect(&addr, &handle)
                        .map(|s| new_line_transport(s))
                        .and_then(|s| {
                            Box::new(Client { service: LineClient::new(s) }) as Box<Future<Item=AsyncService<Request=String,Error=io::Error>,Error=io::Error>>
                        })
                }));*/

                let factory = self.factory.clone();

                let mut f = Box::new(delay.and_then(move |_| {
                    TcpStream::connect(&addr, &handle)
                        .and_then(move |s| futures::done(factory.new_service(s)))
                }))
                as Box<Future<Item=S,Error=io::Error>>;

                try!(f.poll().map_err(|e| {
                    debug!("error connecting 1");
                    e
                }));

                ConnStream::Connecting::<S>(f)
            },
            ConnStream::Connecting(mut f) => {
                trace!("connecting to {}", self.addr);

                match f.poll() {
                    Ok(Async::Ready(mut stream)) => {
                        match self.write_to_stream(&mut stream) {
                            Ok(()) => ConnStream::Connected(stream),
                            Err(e) => {
                                debug!("error writing to stream 1");
                                ConnStream::NotConnected
                            }
                        }
                    },
                    Ok(Async::NotReady) => {
                        ConnStream::Connecting(f)
                    },
                    Err(e) => {
                        debug!("error connecting 2");
                        ConnStream::NotConnected
                    }
                }
            },
            ConnStream::Connected(mut stream) => {
                trace!("connected to {}", self.addr);

                match self.write_to_stream(&mut stream) {
                    Ok(()) => ConnStream::Connected(stream),
                    Err(e) => {
                        debug!("error writing to stream 2");
                        ConnStream::NotConnected
                    }
                }
            }
        })
    }
}

impl <S> Future for Conn<S> where S: AsyncService<Request=String, Error=io::Error> + 'static {
    type Item=();
    type Error=io::Error;

    fn poll(&mut self) -> Poll<(), io::Error> {
        let taken_stream = self.stream.take().unwrap();

        self.stream = Some(try!(self.poll_stream(taken_stream)));

        if let Some(ConnStream::NotConnected) = self.stream {
            self.stream = Some(try!(self.poll_stream(ConnStream::NotConnected)));
        }

        Ok(Async::NotReady)
    }
}
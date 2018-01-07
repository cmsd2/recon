use tokio_core::reactor::Handle;
use futures::{self, Async, Future, Stream, Sink, Poll};
use futures::sync::mpsc::{channel, Sender, Receiver};
use std::io;
use std::net::{SocketAddr, TcpListener};
use ::framing::*;

pub trait AsyncService {
    type Request;
    type Error;
    fn send(&self, req: Self::Request) -> io::Result<()>;
    fn poll(&mut self) -> Poll<(), Self::Error>;
}

impl <R,E> Future for AsyncService<Request=R, Error=E> {
    type Item=();
    type Error=E;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        AsyncService::poll(self)
    }
}

pub trait NewService {
    type Request;
    type Error;
    type Item;

    fn new_service(&self) -> io::Result<Self::Item>;
}

pub trait Service {
    /// Requests handled by the service.
    type Request;

    /// Errors produced by the service.
    type Error;

    /// The future response value.
    type Future: Future<Item = (), Error = Self::Error>;

    /// Process the request and return the response asynchronously.
    fn call(&self, req: Self::Request) -> Self::Future;

    /// Returns `Async::Ready` when the service is ready to accept a request.
    fn poll_ready(&self) -> Async<()>;
}

#[derive(Clone)]
pub struct Funnel {
    channel: Sender<String>,
}

impl Funnel {
    pub fn new(chan: Sender<String>) -> Funnel {
        Funnel {
            channel: chan
        }
    }
}

impl NewService for Funnel {
    type Request = String;
    type Error = io::Error;
    type Item = Self;

    fn new_service(&self) -> io::Result<Funnel> {
        Ok(self.clone())
    }
}

impl Service for Funnel {
    type Request = String;
    type Error = io::Error;
    type Future = Box<Future<Item=(), Error=Self::Error>>;

    fn call(&self, req: String) -> Self::Future {
        // don't get Sink::send mixed up with Sender::send
        let res: io::Result<()> = Sender::send(&self.channel, req);

        Box::new(futures::done(res))
    }

    fn poll_ready(&self) -> Async<()> {
        Async::Ready(())
    }
}

pub struct Server<S, T> where S: Service<Request=String, Error=io::Error>, 
        T: Stream<Item=ReconFrame, Error=io::Error> + Sink<SinkItem=ReconFrame, SinkError=io::Error> {
    service: S,
    transport: T
}

impl <S, T> Server<S, T> where S: Service<Request=String,Error=io::Error>, 
            T: Stream<Item=ReconFrame, Error=io::Error> + Sink<SinkItem=ReconFrame, SinkError=io::Error> {
    pub fn new(service: S, transport: T) -> Server<S, T> {
        Server {
            service: service,
            transport: transport,
        }
    }
}

impl<S, T> Future for Server<S, T>
    where T: Stream<Item=ReconFrame, Error=io::Error> + Sink<SinkItem=ReconFrame, SinkError=io::Error>,
          S: Service<Request = String, Error = T::Error> + 'static
{
    type Item = ();
    type Error = io::Error;

    fn poll(&mut self) -> Poll<(), io::Error> {
        loop {
            match self.transport.poll() {
                Ok(Async::Ready(None)) => {
                    info!("transport eof");
                    return Ok(Async::Ready(()))
                },
                Ok(Async::Ready(Some(item))) => {
                    match item {
                        ReconFrame::Message(s) => {
                            trace!("calling service with frame {:?}", s);
                            self.service.call(s);
                        },
                        ReconFrame::Done => {
                            debug!("service got end frame");
                            return Ok(Async::Ready(()))
                        }
                    }
                },
                Ok(Async::NotReady) => {
                    return Ok(Async::NotReady)
                },
                Err(e) => {
                    return Err(e)
                }
            }
        }
    }
}

pub struct ServerHandle {

}

pub fn listen<F>(handle: &Handle, addr: SocketAddr, f: F) -> Result<ServerHandle> where F: FnMut<(), ()> {
    let listener = TcpListener::bind(addr)?;

    Ok(ServerHandle{})
}

/// Serve a service up. Secret sauce here is 'NewService', a helper that must be able to create a
/// new 'Service' for each connection that we receive.
pub fn serve<T,S>(handle: &Handle,  addr: SocketAddr, new_service: T)
                -> io::Result<ServerHandle>
    where T: NewService<Request = String, Error = io::Error, Item=S> + Send + 'static,
    S: Service<Request=String, Error=io::Error> + 'static,
    S::Future: Future<Item=(),Error=io::Error>
{
    let server_handle = try!(listen(handle, addr, move |stream| {
        // Initialize the pipeline dispatch with the service and the line
        // transport
        let service = try!(new_service.new_service());
        Ok(Server::new(service, new_line_transport(stream)))
    }));

    Ok(server_handle)
}
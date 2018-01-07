use futures::{self, Async, Future, Sink, Stream, AsyncSink, StartSend, Poll};
use std::io;
use std::net::SocketAddr;
use std::collections::VecDeque;
use tokio_core::reactor::Handle;
use tokio_core::net::TcpStream;
use tokio_timer::*;
use std::cell::RefCell;
use std::mem;
use std::time::Duration;

use super::service::*;
use super::framing::{new_line_transport, FramedLineTransport, ReconFrame};
use super::fut::*;

pub trait NewClient where Self::Item: AsyncService<Request=Self::Request, Error=Self::Error>, Self::Io: Io {
    type Request;
    type Error;
    type Item;
    type Io;

    fn new_service(&self, io: Self::Io) -> io::Result<Self::Item>;

    fn box_clone(&self) -> Box<NewClient<Request=Self::Request,Error=Self::Error,Item=Self::Item,Io=Self::Io>>;
}

impl <R,I,O> Clone for Box<NewClient<Request=R,Error=io::Error,Item=I,Io=O>> where I: AsyncService<Request=R,Error=io::Error>, O: Io {
    fn clone(&self) -> Box<NewClient<Request=R,Error=io::Error,Item=I,Io=O>> {
        self.box_clone()
    }
}

/// And the client handle.
pub struct LineClient<S> where S: Sink<SinkError=io::Error> + 'static {
    inner: RefCell<S>,
    //reqs: RefCell<VecDeque<S::Future>>,
}

impl <S> LineClient<S> where S: Sink<SinkError=io::Error> + 'static {
    pub fn new(inner: S) -> LineClient<S> {
        LineClient {
            inner: RefCell::new(inner),
            //reqs: RefCell::new(VecDeque::new()),
        }
    }
}

impl <S> AsyncService for LineClient<S> where S: Sink<SinkError=io::Error> + 'static {
    type Request = S::SinkItem;
    type Error = io::Error;

    fn send(&self, req: Self::Request) -> io::Result<()> {
        trace!("lineclient sending request");
        //self.reqs.borrow_mut().push_back(self.inner.send(req));
        let mut sink = self.inner.borrow_mut();
        
        if let AsyncSink::NotReady(req) = try!(sink.start_send(req)
            .map_err(|e| {
                debug!("error sending to sink");
                io::Error::new(io::ErrorKind::Other, "sink start_send error")
            })
        ) {
            debug!("sink not ready for send");
        }

        Ok(())
    }

    fn poll(&mut self) -> Poll<(), Self::Error> {
        trace!("polling lineclient");

        try!(self.inner.borrow_mut().poll_complete().map_err(|e| {
            debug!("error polling sink completion");
            io::Error::new(io::ErrorKind::Other, "sink error")
        }));

        Ok(Async::NotReady)
    }
}

pub struct Client<S> where S: AsyncService<Request=ReconFrame, Error=io::Error> {
    service: S
}

impl <S> Client<S> where S: AsyncService<Request=ReconFrame, Error=io::Error> {
    pub fn new(inner: S) -> Client<S> {
        Client {
            service: inner
        }
    }
}

impl <S> AsyncService for Client<S> where S: AsyncService<Request=ReconFrame, Error=io::Error> + 'static {
    type Request = String;
    type Error = io::Error;

    fn send(&self, req: String) -> io::Result<()> {
        if req.chars().find(|&c| c == '\n').is_some() {
            debug!("failure: message contained newline");
            let err = io::Error::new(io::ErrorKind::InvalidInput, "message contained new line");
            return Err(err);
        }

        trace!("sending message");

        self.service.send(ReconFrame::Message(req))
    }

    fn poll(&mut self) -> Poll<(), Self::Error> {
        self.service.poll()
    }
}

#[derive(Clone)]
pub struct NewLineClient { 
    handle: Handle
}

impl NewLineClient {
    pub fn new(handle: Handle) -> NewLineClient {
        NewLineClient {
            handle: handle,
        }
    }
}

impl NewClient for NewLineClient {
    type Request=String;
    type Error=io::Error;
    type Item=Client<LineClient<FramedLineTransport<TcpStream>>>;
    type Io=TcpStream;

    fn new_service(&self, io: TcpStream) -> io::Result<Self::Item> {
        Ok(Client::new(LineClient::new(new_line_transport(io))))
    }

    fn box_clone(&self) -> Box<NewClient<Request=Self::Request,Error=Self::Error,Item=Self::Item,Io=Self::Io>> {
        Box::new(self.clone())
    }
}

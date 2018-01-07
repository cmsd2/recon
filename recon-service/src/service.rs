use futures::{Async, Future, Poll};

pub trait AsyncService {
    type Request;
    type Error;

    fn send(&self, req: Self::Request) -> Result<(), Self::Error>;
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
    type Error;
    type Item;

    fn new_service(&self) -> Result<Self::Item, Self::Error>;
}

pub trait Service {
    /// Requests handled by the service.
    type Request;

    /// Errors produced by the service.
    type Error;

    /// The future response value.
    type Future: Future<Item = (), Error = Self::Error>;

    /// Process the request and return the response asynchronously.
    fn post(&self, req: Self::Request) -> Self::Future;

    /// Returns `Async::Ready` when the service is ready to accept a request.
    fn poll_ready(&self) -> Async<()>;
}

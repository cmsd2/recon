use futures::*;
use std::cell::RefCell;
use std::sync::Arc;

pub struct CompletableFuture<T, E> {
    value: Arc<RefCell<Option<Result<T,E>>>>
}

impl <T, E> Clone for CompletableFuture<T,E> {
    fn clone(&self) -> CompletableFuture<T,E> {
        CompletableFuture {
            value: self.value.clone()
        }
    }
}

impl <T, E> CompletableFuture<T, E> {
    pub fn new() -> CompletableFuture<T, E> {
        CompletableFuture {
            value: Arc::new(RefCell::new(None))
        }
    }

    pub fn resolve(&self, value: T) {
        *self.value.borrow_mut() = Some(Ok(value));
    }

    pub fn fail(&self, err: E) {
        *self.value.borrow_mut() = Some(Err(err));
    }
}

impl <T,E> Future for CompletableFuture<T,E> {
    type Item=T;
    type Error=E;

    fn poll(&mut self) -> Poll<T, E> {
        match self.value.borrow_mut().take() {
            None => Ok(Async::NotReady),
            Some(Ok(v)) => Ok(Async::Ready(v)),
            Some(Err(e)) => Err(e)
        }
    }
}
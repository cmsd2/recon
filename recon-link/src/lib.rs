#[macro_use]
extern crate log;
#[macro_use]
extern crate futures;
extern crate tokio_core;
extern crate tokio_io;
extern crate tokio_retry;
extern crate tokio_timer;
extern crate tokio_codec;
#[macro_use]
extern crate state_machine_future;
extern crate bytes;

pub mod conn;
pub mod framing;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}

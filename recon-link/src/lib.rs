#[macro_use]
extern crate log;
#[macro_use]
extern crate futures;
extern crate tokio_core;
extern crate tokio_io;
extern crate tokio_retry;
extern crate tokio_timer;
#[macro_use]
extern crate state_machine_future;
extern crate bytes;
extern crate uuid;
extern crate recon_util;

pub mod conn;
pub mod framing;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}

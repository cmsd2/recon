#[macro_use]
extern crate log;
#[cfg(feature="logger")]
extern crate env_logger;
extern crate futures;
extern crate uuid;

pub mod codec;
pub mod pub_sub;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}

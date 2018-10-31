use tokio_io::codec::{Decoder, Encoder};
use std::io;
use bytes::{BytesMut};

pub struct Codec;

pub type Request = String;
pub type Response = String;
pub type Error = io::Error;

impl Decoder for Codec {
    type Item = Request;
    type Error = Error;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        trace!("buffer has {} bytes available for decoding", buf.len());

        if let Some(n) = buf.iter().position(|b| *b == b'\n') {
            // remove this line and the newline from the buffer.
            let line = buf.split_to(n);
            buf.split_to(1);

            trace!("buffer has {} bytes remaining after decoding", buf.len());

            // Turn this data into a UTF string and return it in a Frame.
            return match std::str::from_utf8(&line[..]) {
                Ok(msg) => {
                    Ok(Some(msg.to_string()))
                },
                Err(_) => Err(io::Error::new(io::ErrorKind::Other, "invalid string"))
            }
        }

        Ok(None)
    }
}

impl Encoder for Codec {
    type Item = Response;
    type Error = Error;

    fn encode(
        &mut self, msg: Response, buf: &mut BytesMut,
    ) -> Result<(), Self::Error> {

        if msg.contains('\n') {
            return Err(io::Error::new(io::ErrorKind::Other, "line transport can't handle newlines"));
        }

        buf.extend_from_slice(&msg.as_bytes());
        buf.extend_from_slice(&['\n' as u8]);

        Ok(())
    }
}
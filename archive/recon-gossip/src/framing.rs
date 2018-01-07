use bytes::*;
use futures::{Async, Poll};
use std::{io, str};
use std::io::Write;
use std::fmt;
use tokio_io::AsyncRead;
use tokio_io::codec::{BytesCodec, Framed, Encoder, Decoder};

#[derive(Debug,  Clone)]
pub enum ReconFrame {
    Message(String),
    Done,
}

pub struct Parser;

impl Decoder for Parser {
    type Item=ReconFrame;
    type Error=io::Error;

    fn decode(&mut self, buf: &mut Bytes) -> Result<Option<ReconFrame>, io::Error> {
        // If our buffer contains a newline...
        trace!("buffer has {} bytes available for decoding", buf.len());

        if let Some(n) = buf.as_slice().iter().position(|b| *b == b'\n') {
            let mut buf = buf.get_mut();
            // remove this line and the newline from the buffer.
            let line: Vec<u8> = buf.drain(0..n).collect();
            buf.remove(0);

            trace!("buffer has {} bytes remainng after decoding", buf.len());

            // Turn this data into a UTF string and return it in a Frame.
            return match str::from_utf8(&line[..]) {
                Ok(s) => Ok(Some(ReconFrame::Message(s.to_string()))),
                Err(_) => Err(io::Error::new(io::ErrorKind::Other, "invalid string"))
           }
        }

        Ok(None)
    }

    fn decode_eof(&mut self, buf: &mut Bytes) -> Result<ReconFrame, io::Error> {
        if buf.len() == 0 {
            Ok(ReconFrame::Done)
        } else {
            Err(io::Error::new(io::ErrorKind::InvalidData, "trailing data found"))
        }
    }
}

impl Encoder for Parser {
    type Item=ReconFrame;
    type Error=io::Error;

    fn encode(&mut self, msg: ReconFrame, buf: &mut Vec<u8>) -> Result<(), io::Error> {
        match msg {
            ReconFrame::Message(text) => {
                buf.copy_from_slice(&text.as_bytes());
                buf.copy_from_slice(&['\n' as u8]);
            }
            ReconFrame::Done => {}
        }

        Ok(())
    }
}

pub type FramedLineTransport<T> = Framed<T, Parser>;

pub fn new_line_transport<T>(inner: T) -> FramedLineTransport<T>
    where T: AsyncRead,
{
  inner.framed(Parser)
}

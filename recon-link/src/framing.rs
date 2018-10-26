use bytes::BytesMut;
use std::{io, str};
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_codec::{Framed, Encoder, Decoder};

#[derive(Debug, Clone)]
pub enum ReconFrame {
    Message(String),
    Done,
}

pub struct Parser;

impl Decoder for Parser {
    type Item=ReconFrame;
    type Error=io::Error;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<ReconFrame>, io::Error> {
        trace!("buffer has {} bytes available for decoding", buf.len());

        if let Some(n) = buf.iter().position(|b| *b == b'\n') {
            // remove this line and the newline from the buffer.
            let line = buf.split_to(n);
            buf.split_to(1);

            trace!("buffer has {} bytes remaining after decoding", buf.len());

            // Turn this data into a UTF string and return it in a Frame.
            return match str::from_utf8(&line[..]) {
                Ok(msg) => {
                    Ok(Some(ReconFrame::Message(msg.to_string())))
                },
                Err(_) => Err(io::Error::new(io::ErrorKind::Other, "invalid string"))
           }
        }

        Ok(None)
    }

    fn decode_eof(&mut self, buf: &mut BytesMut) -> Result<Option<ReconFrame>, io::Error> {
        if buf.len() == 0 {
            //Ok(Some(ReconFrame::Done))
            Err(io::Error::new(io::ErrorKind::UnexpectedEof, "stream eof"))
        } else {
            Err(io::Error::new(io::ErrorKind::InvalidData, "trailing data found"))
        }
    }
}

impl Encoder for Parser {
    type Item=ReconFrame;
    type Error=io::Error;

    fn encode(&mut self, msg: ReconFrame, buf: &mut BytesMut) -> Result<(), io::Error> {
        match msg {
            ReconFrame::Message(text) => {
                if text.contains('\n') {
                    return Err(io::Error::new(io::ErrorKind::Other, "line transport can't handle newlines"));
                }

                buf.extend_from_slice(&text.as_bytes());
                buf.extend_from_slice(&['\n' as u8]);
            }
            ReconFrame::Done => {}
        }

        Ok(())
    }
}

pub type FramedLineTransport<T> = Framed<T, Parser>;

pub fn new_line_transport<T>(inner: T) -> FramedLineTransport<T>
    where T: AsyncRead + AsyncWrite,
{
    FramedLineTransport::<T>::new(inner, Parser)
}

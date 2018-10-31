use actix::prelude::*;
use actix::io::{FramedWrite};
use tokio_io::AsyncRead;
use tokio_io::io::WriteHalf;
use tokio_codec::FramedRead;
use tokio_tcp::TcpStream;
use codec::Codec;

#[derive(Message)]
pub struct Post {
    pub message: ::codec::Request,
}

pub struct Link {
    writer: FramedWrite<WriteHalf<TcpStream>,Codec>,
}

impl Link {
    pub fn new(tcp_stream: TcpStream) -> actix::Addr<Link> {
        Link::create(move |ctx| {
            let (r, w) = tcp_stream.split();
            Link::add_stream(FramedRead::new(r, Codec), ctx);
            Link {
                writer: FramedWrite::new(w, Codec, ctx)
            }
        })
    }
}

impl Actor for Link {
    type Context = Context<Self>;
}

impl actix::io::WriteHandler<::codec::Error> for Link {}

impl StreamHandler<::codec::Request, ::codec::Error> for Link {
    fn handle(&mut self, msg: ::codec::Request, _ctx: &mut Self::Context) {
        debug!("received message {:?}", msg);
    }
}

impl Handler<Post> for Link {
    type Result = ();

    fn handle(&mut self, msg: Post, _ctx: &mut Self::Context) -> Self::Result {
        self.writer.write(msg.message)
    }
}
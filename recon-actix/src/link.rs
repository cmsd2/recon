use futures::Future;
use actix::prelude::*;
use actix::io::{FramedWrite};
use tokio_io::AsyncRead;
use tokio_io::io::WriteHalf;
use tokio_codec::FramedRead;
use tokio_tcp::TcpStream;
use std::net::SocketAddr;
use codec::Codec;

pub type LinkError = ::codec::Error;

#[derive(Message)]
pub struct Post {
    pub message: ::codec::Request,
}

pub struct Link {
    connection_manager: Recipient<LinkStopped>,
    writer: FramedWrite<WriteHalf<TcpStream>,Codec>,
    error: Option<LinkError>,
    id: String,
    peer_addr: SocketAddr,
}

#[derive(Message,Debug)]
pub struct LinkStopped {
    pub reason: LinkStoppedReason,
    pub id: String,
    pub peer_addr: SocketAddr,
}

#[derive(Debug)]
pub enum LinkStoppedReason {
    Finished,
    Error(LinkError)
}

impl Link {
    pub fn new(tcp_stream: TcpStream, id: String, connection_manager: Recipient<LinkStopped>) -> actix::Addr<Link> {
        Link::create(move |ctx| {
            let peer_addr = tcp_stream.peer_addr().expect("error getting socket peer address");
            let (r, w) = tcp_stream.split();
            Link::add_stream(FramedRead::new(r, Codec), ctx);
            Link {
                connection_manager: connection_manager,
                writer: FramedWrite::new(w, Codec, ctx),
                id: id,
                peer_addr: peer_addr,
                error: None,
            }
        })
    }
}

impl Actor for Link {
    type Context = Context<Self>;
}

impl actix::io::WriteHandler<LinkError> for Link {}

impl StreamHandler<::codec::Request, LinkError> for Link {
    fn handle(&mut self, msg: ::codec::Request, _ctx: &mut Self::Context) {
        debug!("received message {:?}", msg);
    }

    fn error(&mut self, err: LinkError, ctx: &mut Self::Context) -> Running {
        self.error = Some(err);
        Running::Stop
    }

    fn finished(&mut self, ctx: &mut Self::Context) {
        let reason = if let Some(err) = self.error.take() {
            LinkStoppedReason::Error(err)
        } else {
            LinkStoppedReason::Finished
        };
        let id = self.id.clone();
        let peer_addr = self.peer_addr.clone();

        Arbiter::spawn(self.connection_manager
            .send(LinkStopped { reason, id, peer_addr })
            .map_err(|err| {
                warn!("error sending link stopped message: {}", err);
                ()
            }));

        ctx.stop()
    }
}

impl Handler<Post> for Link {
    type Result = ();

    fn handle(&mut self, msg: Post, _ctx: &mut Self::Context) -> Self::Result {
        self.writer.write(msg.message)
    }
}
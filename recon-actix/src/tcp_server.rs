use actix::prelude::*;
use futures::Stream;
use tokio_tcp::{TcpListener, TcpStream};
use ::link::Link;
use ::connection_table::{AddListener,ConnectionState,Connection,ConnectionEvent,ConnectionTable,Event};

pub struct TcpServer {
    connections: Addr<ConnectionTable>,
}

impl TcpServer {
    pub fn new(tcp_listener: TcpListener, connections: Addr<ConnectionTable>) -> actix::Addr<TcpServer> {
        let connections2 = connections.clone();
        let addr = TcpServer::create(|ctx| {
            ctx.add_stream(
                tcp_listener.incoming()
                        .map_err(|_| ())
                        .map(|stream| TcpConnect { stream }));
            TcpServer {
                connections: connections2,
            }
        });
        connections.do_send(AddListener(addr.clone().recipient()));
        addr
    }
}

impl Actor for TcpServer {
    type Context = Context<Self>;
}

#[derive(Message)]
pub struct TcpConnect {
    pub stream: TcpStream,
}
type TcpConnectError = ();

impl StreamHandler<TcpConnect,TcpConnectError> for TcpServer {
    fn handle(&mut self, msg: TcpConnect, _ctx: &mut Context<Self>) {
        info!("tcp connect event from {:?}", msg.stream.peer_addr().unwrap());

        let _addr = Link::new(msg.stream);
    }

    fn started(&mut self, _ctx: &mut Context<Self>) {
        info!("stream handler started.");
    }

    fn error(&mut self, _err: TcpConnectError, _ctx: &mut Context<Self>) -> Running {
        info!("tcp connect error. continuing.");
        Running::Continue
    }

    fn finished(&mut self, _ctx: &mut Context<Self>) {
        info!("stream handler finished");
    }
}

impl Handler<ConnectionEvent> for TcpServer {
    type Result = ();

    fn handle(&mut self, msg: ConnectionEvent, _ctx: &mut Context<Self>) -> Self::Result {
        match msg.event {
            Event::Added => {
                debug!("starting new connection actor for {:?}", msg.connection);
            },
            Event::Updated => {
                debug!("connection table entry updated {:?}", msg.connection);
            },
            Event::Removed => {
                debug!("stopping connection actor for {:?}", msg.connection);
            }
        }
    }
}
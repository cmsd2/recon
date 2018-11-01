use actix::prelude::*;
use futures::{Future, Stream};
use tokio_tcp::{TcpListener, TcpStream};
use ::link::{Link,LinkStopped};
use ::connection_manager::*;
use ::connection_table::ConnectionTable;

pub struct TcpServer {
    connections: ConnectionTable,
}

impl TcpServer {
    pub fn new(tcp_listener: TcpListener) -> actix::Addr<TcpServer> {
        let addr = TcpServer::create(|ctx| {
            ctx.add_stream(
                tcp_listener.incoming()
                        .map_err(|_| ())
                        .map(|stream| TcpConnect { stream }));
            TcpServer {
                connections: ConnectionTable::new(),
            }
        });
        addr
    }

    pub fn connect(&mut self, connection: Connection, ctx: &mut Context<Self>) {
        debug!("connecting to {:?}", connection);
        let stream_fut = TcpStream::connect(&connection.addr);
        let listener = ctx.address().recipient();
        let error_listener = ctx.address();
        self.connections.update_connection_state(UpdateConnectionState {
            id: connection.id,
            state: ConnectionState::Connecting,
        });
        Arbiter::spawn(stream_fut.map(|stream| {
            Link::new(stream, connection.id, listener);
        }).map_err(|err| {
            debug!("error connecting to peer: {}", err);
            Arbiter::spawn(error_listener.send(LinkConnectError {
                id: connection.id,
                tries: connection.reconnect_tries,
            }).map_err(|err| {
                warn!("error sending link connect error message");
            }));
            ()
        }));
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

#[derive(Message,Debug)]
pub struct LinkConnectError {
    id: String,
    tries: u32,
}

impl Handler<LinkConnectError> for TcpServer {
    type Result = ();

    fn handle(&mut self, msg: LinkConnectError, ctx: &mut Context<Self>) {
        debug!("error connecting. retrying with backoff. {:?}", msg);
        //self.connections.update_connection_state(UpdateConnectionState::ReconnectBackOff)
    }
}

impl StreamHandler<TcpConnect,TcpConnectError> for TcpServer {
    fn handle(&mut self, msg: TcpConnect, ctx: &mut Context<Self>) {
        let peer_addr = msg.stream.peer_addr().expect("error getting socket peer address");
        info!("tcp connect event from {:?}", peer_addr);
        
        let c = self.connections.add_connection(AddConnection {
            addr: peer_addr,
            reconnect: false,
        }).expect("error adding connection");
        
        self.connections.update_connection_state(UpdateConnectionState {
            id: c.id.clone(),
            state: ConnectionState::Connected,
        }).expect("error updating connection state");

        let _addr = Link::new(msg.stream, c.id, ctx.address().recipient());
    }

    fn started(&mut self, _ctx: &mut Context<Self>) {
        info!("stream handler started.");
    }

    fn error(&mut self, _err: TcpConnectError, _ctx: &mut Context<Self>) -> Running {
        info!("tcp connect error. stopping.");
        Running::Stop
    }

    fn finished(&mut self, ctx: &mut Context<Self>) {
        info!("stream handler finished");
        ctx.stop();
    }
}

impl Handler<LinkStopped> for TcpServer {
    type Result = ();

    fn handle(&mut self, msg: LinkStopped, _ctx: &mut Context<Self>) -> Self::Result {
        info!("link stopped: {:?}", msg);
        self.connections.update_connection_state(UpdateConnectionState {
            id: msg.id,
            state: ConnectionState::NotConnected,
        }).expect("error updating connection state");
    }
}

impl Handler<ConnectionEvent> for TcpServer {
    type Result = ();

    fn handle(&mut self, msg: ConnectionEvent, ctx: &mut Context<Self>) -> Self::Result {
        match msg.event {
            Event::Added => {
                debug!("new connection added {:?}", msg.connection);
                if msg.connection.state == ConnectionState::NotConnected && msg.connection.reconnect {
                    debug!("starting new connection actor");
                    self.connect(msg.connection, ctx);
                }
            },
            Event::Updated => {
                debug!("connection table entry updated {:?}", msg.connection);
                if msg.connection.state == ConnectionState::NotConnected && msg.connection.reconnect {
                    debug!("starting new connection actor");
                    self.connect(msg.connection, ctx);
                }
            },
            Event::Removed => {
                debug!("stopping connection actor for {:?}", msg.connection);
            }
        }
    }
}

impl Handler<AddConnection> for TcpServer {
    type Result = ();

    fn handle(&mut self, msg: AddConnection, _ctx: &mut Context<Self>) -> Self::Result {
        info!("adding new connection {:?}", msg);
        //TODO fix unwrap
        self.connections.add_connection(msg).unwrap();
    }
}

impl Handler<RemoveConnection> for TcpServer {
    type Result = ();

    fn handle(&mut self, msg: RemoveConnection, _ctx: &mut Context<Self>) -> Self::Result {
        info!("removing connection {:?}", msg);
        //TODO fix unwrap
        self.connections.remove_connection(msg).unwrap();
    }
}

impl Handler<GetConnections> for TcpServer {
    type Result = Result<Vec<Connection>,()>;

    fn handle(&mut self, msg: GetConnections, _ctx: &mut Context<Self>) -> Self::Result {
        info!("getting connections");
        Ok(self.connections.get_connections())
    }
}
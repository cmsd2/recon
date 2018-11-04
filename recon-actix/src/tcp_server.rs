use actix::prelude::*;
use connection_manager::*;
use connection_table::ConnectionTable;
use futures::{Future, Stream};
use link::{Link, LinkStopped};
use std::io;
use std::time::Duration;
use tokio_retry::strategy::*;
use tokio_retry::{Action, Retry};
use tokio_tcp::{TcpListener, TcpStream};

pub struct TcpServer {
    connections: ConnectionTable,
}

#[derive(Clone)]
pub struct Connector {
    pub tcp_server: Addr<TcpServer>,
    pub connection: Connection,
    pub tries: usize,
}

impl Connector {
    pub fn new(tcp_server: Addr<TcpServer>, connection: Connection) -> Connector {
        Connector {
            tcp_server: tcp_server,
            connection: connection,
            tries: 0,
        }
    }

    pub fn connect_fut(&mut self) -> impl Future<Item = (), Error = ()> {
        let stream_fut = TcpStream::connect(&self.connection.addr);
        let listener = self.tcp_server.clone().recipient();
        let c1 = self.connection.clone();
        self.tries += 1;
        let tries = self.tries;

        stream_fut
            .map(move |stream| {
                Link::new(stream, c1.id, listener);
            }).map_err(move |err| {
                debug!("try {}. error connecting to peer: {}", tries, err);
            })
    }

    pub fn connect<I>(&self, retry_strategy: I) -> impl Future<Item = (), Error = io::Error>
    where
        I: Iterator<Item = Duration>,
    {
        Retry::spawn(retry_strategy, self.clone()).map_err(|err| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("error spawning connection with retry: {:?}", err),
            )
        })
    }
}

impl Action for Connector {
    type Item = ();
    type Error = ();
    type Future = Box<Future<Item = Self::Item, Error = Self::Error>>;

    fn run(&mut self) -> Self::Future {
        Box::new(self.connect_fut())
    }
}

impl TcpServer {
    pub fn new(tcp_listener: TcpListener) -> actix::Addr<TcpServer> {
        let addr = TcpServer::create(|ctx| {
            ctx.add_stream(
                tcp_listener
                    .incoming()
                    .map_err(|_| ())
                    .map(|stream| TcpConnect { stream }),
            );
            TcpServer {
                connections: ConnectionTable::new(),
            }
        });
        addr
    }

    pub fn connect(&mut self, connection: Connection, ctx: &mut Context<Self>) {
        debug!("connecting to {:?}", connection);
        let error_listener = ctx.address();
        let connected_listener = ctx.address();
        let c1 = connection.clone();
        let c2 = connection.clone();

        Self::notify_connecting(
            ctx.address(),
            LinkConnecting {
                id: connection.id.clone(),
            },
        );

        let connector = Connector::new(ctx.address(), connection.clone());

        let tries = 10;
        let strategy = FibonacciBackoff::from_millis(10).map(jitter).take(tries);
        let fut = connector
            .connect(strategy)
            .map(|()| {
                TcpServer::notify_connected(connected_listener, LinkConnected { id: c1.id });
            }).map_err(move |err| {
                info!("unable to connect after retries. stopping. {:?}", err);
                Self::notify_error(
                    error_listener,
                    LinkConnectError {
                        id: c2.id,
                        tries: tries,
                    },
                );
            });

        Arbiter::spawn(fut);
    }

    fn notify_connecting(tcp_server: Addr<TcpServer>, lc: LinkConnecting) {
        Arbiter::spawn(tcp_server.send(lc).map_err(|err| {
            warn!("error sending link connecting message: {}", err);
        }))
    }

    fn notify_error(tcp_server: Addr<TcpServer>, lce: LinkConnectError) {
        Arbiter::spawn(tcp_server.send(lce).map_err(|err| {
            warn!("error sending link connect error message: {}", err);
        }));
    }

    fn notify_connected(tcp_server: Addr<TcpServer>, lc: LinkConnected) {
        Arbiter::spawn(tcp_server.send(lc).map_err(|err| {
            warn!("error sending link connected message: {}", err);
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

#[derive(Message, Debug)]
struct LinkConnectError {
    id: String,
    tries: usize,
}

#[derive(Message, Debug)]
struct LinkConnected {
    id: String,
}

#[derive(Message, Debug)]
struct LinkConnecting {
    id: String,
}

impl StreamHandler<TcpConnect, TcpConnectError> for TcpServer {
    fn handle(&mut self, msg: TcpConnect, ctx: &mut Context<Self>) {
        let peer_addr = msg
            .stream
            .peer_addr()
            .expect("error getting socket peer address");
        info!("tcp connect event from {:?}", peer_addr);

        let c = self
            .connections
            .add_connection(AddConnection {
                addr: peer_addr,
                reconnect: false,
            }).expect("error adding connection");

        self.connections
            .update_connection_state(UpdateConnectionState {
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

impl Handler<LinkConnecting> for TcpServer {
    type Result = ();

    fn handle(&mut self, msg: LinkConnecting, _ctx: &mut Context<Self>) {
        debug!("link connecting: {:?}", msg);
        if let Err(err) = self
            .connections
            .update_connection_state(UpdateConnectionState {
                id: msg.id,
                state: ConnectionState::Connecting,
            }) {
            info!("error updating connection state: {:?}", err);
        }
    }
}

impl Handler<LinkConnected> for TcpServer {
    type Result = ();

    fn handle(&mut self, msg: LinkConnected, _ctx: &mut Context<Self>) {
        debug!("link connected: {:?}", msg);
        self.connections
            .update_connection_state(UpdateConnectionState {
                id: msg.id,
                state: ConnectionState::Connected,
            }).expect("error updating connection state");
    }
}

impl Handler<LinkConnectError> for TcpServer {
    type Result = ();

    fn handle(&mut self, msg: LinkConnectError, _ctx: &mut Context<Self>) {
        debug!("error connecting: {:?}", msg);
        self.connections
            .update_connection_state(UpdateConnectionState {
                id: msg.id,
                state: ConnectionState::NotConnected,
            }).expect("error updating connection state");
    }
}

impl Handler<LinkStopped> for TcpServer {
    type Result = ();

    fn handle(&mut self, msg: LinkStopped, _ctx: &mut Context<Self>) -> Self::Result {
        info!("link stopped: {:?}", msg);
        self.connections
            .update_connection_state(UpdateConnectionState {
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
                if msg.connection.state == ConnectionState::NotConnected && msg.connection.reconnect
                {
                    debug!("starting new connection actor");
                    self.connect(msg.connection, ctx);
                }
            }
            Event::Updated => {
                debug!("connection table entry updated {:?}", msg.connection);
                if msg.connection.state == ConnectionState::NotConnected && msg.connection.reconnect
                {
                    debug!("starting new connection actor");
                    self.connect(msg.connection, ctx);
                }
            }
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
    type Result = Result<Vec<Connection>, ()>;

    fn handle(&mut self, _msg: GetConnections, _ctx: &mut Context<Self>) -> Self::Result {
        info!("getting connections");
        Ok(self.connections.get_connections())
    }
}

use actix::prelude::*;
use std::net::SocketAddr;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Event {
    Added,
    Updated,
    Removed,
}

#[derive(Clone, Message, Serialize, Deserialize)]
pub struct ConnectionEvent {
    pub connection: Connection,
    pub event: Event,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ConnectionState {
    NotConnected,
    Connecting,
    Connected,
    ReconnectBackoff,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Connection {
    pub id: String,
    pub addr: SocketAddr,
    pub state: ConnectionState,
    pub reconnect: bool,
    pub reconnect_tries: usize,
}

pub trait ConnectionManager<Context>:
    Actor<Context = Context>
    + Handler<AddConnection>
    + Handler<RemoveConnection>
    + Handler<GetConnections>
where
    Context: ActorContext,
{
}

impl<Context, T> ConnectionManager<Context> for T
where
    T: Actor<Context = Context>
        + Handler<AddConnection>
        + Handler<RemoveConnection>
        + Handler<GetConnections>,
    Context: ActorContext,
{}

#[derive(Message, Debug, Serialize, Deserialize)]
pub struct AddConnection {
    pub addr: SocketAddr,
    pub reconnect: bool,
}

#[derive(Message, Debug, Serialize, Deserialize)]
pub struct UpdateConnection {
    pub id: String,
    pub reconnect: bool,
}

#[derive(Message, Debug)]
pub struct UpdateConnectionState {
    pub id: String,
    pub state: ConnectionState,
}

#[derive(Message, Debug)]
pub struct RemoveConnection {
    pub id: String,
}

#[derive(Message, Debug)]
#[rtype(result = "Result<Vec<::connection_manager::Connection>, ()>")]
pub struct GetConnections;

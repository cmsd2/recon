use actix::prelude::*;
use std::net::SocketAddr;
use std::result;
use snowflake::ProcessUniqueId;

#[derive(Clone,Debug,PartialEq,Serialize,Deserialize)]
pub enum Event {
    Added,
    Updated,
    Removed,
}

#[derive(Clone,Message,Serialize,Deserialize)]
pub struct ConnectionEvent {
    pub connection: Connection,
    pub event: Event,
}

#[derive(Debug,Clone,PartialEq,Serialize,Deserialize)]
pub enum ConnectionState {
    NotConnected,
    //Connecting,
    //Connected,
}

#[derive(Debug,Clone,PartialEq,Serialize,Deserialize)]
pub struct Connection {
    pub id: String,
    pub addr: SocketAddr,
    pub state: ConnectionState,
    pub reconnect: bool,
}

pub struct ConnectionTable {
    connections: Vec<Connection>,
    listeners: Vec<Recipient<ConnectionEvent>>,
}

impl ConnectionTable {
    pub fn new() -> ConnectionTable {
        ConnectionTable {
            connections: vec![],
            listeners: vec![],
        }
    }

    pub fn add_listener(&mut self, r: Recipient<ConnectionEvent>) {
        self.listeners.push(r);
    }
}

impl Actor for ConnectionTable {
    type Context = Context<ConnectionTable>;
}

#[derive(Message)]
pub struct AddListener(pub Recipient<ConnectionEvent>);

impl Handler<AddListener> for ConnectionTable {
    type Result = ();

    fn handle(&mut self, msg: AddListener, _ctx: &mut Context<Self>) -> Self::Result {
        self.add_listener(msg.0);
    }
}

#[derive(Message,Serialize,Deserialize)]
pub struct AddConnection {
    pub addr: SocketAddr,
    pub reconnect: bool,
}
impl Handler<AddConnection> for ConnectionTable {
    type Result = ();

    fn handle(&mut self, msg: AddConnection, _ctx: &mut Context<Self>) -> Self::Result {
        for mut c in self.connections.iter_mut() {
            if c.addr == msg.addr {
                c.reconnect = msg.reconnect;
                debug!("updated connection entry {:?}", c);
                //TODO: replace with async
                for r in self.listeners.iter() {
                    r.do_send(ConnectionEvent {
                        connection: c.clone(),
                        event: Event::Updated,
                    }).unwrap();
                }
                return;
            }
        }

        let c = Connection {
            id: format!("{}", ProcessUniqueId::new()),
            addr: msg.addr,
            reconnect: msg.reconnect,
            state: ConnectionState::NotConnected,
        };
        debug!("adding connection {:?}", c);
        self.connections.push(c.clone());

        //TODO: replace with async
        for r in self.listeners.iter() {
            r.do_send(ConnectionEvent {
                connection: c.clone(),
                event: Event::Added,
            }).unwrap();
        }
    }
}

#[derive(Message)]
pub struct RemoveConnection {
    pub id: String,
}
impl Handler<RemoveConnection> for ConnectionTable {
    type Result = ();

    fn handle(&mut self, msg: RemoveConnection, _ctx: &mut Context<Self>) -> Self::Result {
        let listeners = self.listeners.clone();
        self.connections.retain(|c: &Connection| {
            if c.id == msg.id {
                debug!("removing connection {:?}", c);
                //TODO: replace with async
                for r in listeners.iter() {
                    r.do_send(ConnectionEvent {
                        connection: c.clone(),
                        event: Event::Removed,
                    }).unwrap();
                }
                false
            } else {
                true
            }
        });
    }
}

#[derive(Message)]
#[rtype(result="Result<Vec<::connection_table::Connection>, ()>")]
pub struct GetConnections;
impl Handler<GetConnections> for ConnectionTable {
    type Result = result::Result<Vec<Connection>,()>;

    fn handle(&mut self, _msg: GetConnections, _ctx: &mut Context<Self>) -> Self::Result {
        Ok(self.connections.clone())
    }
}
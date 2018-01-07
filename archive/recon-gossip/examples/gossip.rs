extern crate futures;
extern crate tokio_core;
extern crate tokio_proto;
extern crate tokio_service;
extern crate tokio_timer;
#[macro_use]
extern crate slog;
extern crate slog_term;
extern crate slog_stdlog;
extern crate slog_envlogger;
#[macro_use]
extern crate log;
extern crate recon;
extern crate chrono;

use std::cell::RefCell;
use std::net::{self, SocketAddr};
use std::io::{self, Write, BufRead};
use std::fs;
use std::collections::{VecDeque, HashSet};
use futures::{Future, Poll, Async, Stream};
use tokio_core::reactor::{Core, Handle};
use tokio_core::net::{TcpStream};
use tokio_core::io::{copy, write_all, Io};
use tokio_core::channel;
use tokio_service::Service;
use tokio_timer::Timer;
use chrono::Duration;

use recon::client::{NewClient, Client, LineClient, NewLineClient};
use recon::framing::{ReconFrame, FramedLineTransport, new_line_transport};
use recon::service::{AsyncService};
use recon::conn::*;
use recon::link::{Link};
use recon::multiplex::{self, MultiplexLink};
use recon::watchdog::{Watchdog};
use recon::upb::{UnreliableProbabilisticBroadcast};
use recon::lpb::{LazyProbabilisticBroadcast};

struct App {
    delay: Option<Box<Future<Item=(), Error=io::Error>>>,
    interval: Duration,
    seq: u32,
    link: MultiplexLink,
    link_rx: channel::Receiver<multiplex::Message>,
    multiplex_key: String,
    gossip: LazyProbabilisticBroadcast<String>,
}

impl App {
    pub fn new(handle: Handle, id: String, interval: Duration, addr: SocketAddr, conn_factory: NewLineClient, 
            peers: HashSet<String>, k: usize, rounds: u32, alpha: f32, delta: Duration) -> io::Result<App> {
        let (link_tx, link_rx) = try!(channel::channel(&handle));

        let link = try!(MultiplexLink::new(handle.clone(), id.clone(), addr, conn_factory));
        let gossip = try!(LazyProbabilisticBroadcast::new(
            handle.clone(), id.clone(), peers, link.channel(), k, rounds, alpha, delta, 
            "gossip".to_owned(), delta * 2));

        let key = "app";
        try!(link.subscribe(key, link_tx));

        Ok(App {
            delay: Some(try!(Self::delay(interval))),
            interval: interval,
            seq: 0,
            link: link,
            link_rx: link_rx,
            multiplex_key: key.to_string(),
            gossip: gossip,
        })
    }

    pub fn delay(interval: Duration) -> Result<Box<Future<Item=(), Error=io::Error>>, io::Error> {
        let std_interval = try!(interval.to_std()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, "duration out of range")));

        Ok(Box::new(Timer::default()
            .sleep(std_interval)
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "timer error")))
            as Box<Future<Item=(), Error=io::Error>>)
    }

    pub fn add_conn(&self, client_id: &str, addr: SocketAddr) -> io::Result<()> {
        self.link.add(client_id, addr)
    }
}

impl Future for App {
    type Item=();
    type Error=io::Error;

    fn poll(&mut self) -> Poll<(), io::Error> {
        let mut delay = self.delay.take().unwrap();

        match try!(delay.poll()) {
            Async::Ready(()) => {
                self.seq += 1;
                let value = format!("hello {}", self.seq);
                try!(self.gossip.broadcast(value));

                self.delay = Some(try!(Self::delay(self.interval)));
                try!(self.poll());
            },
            Async::NotReady => {
                self.delay = Some(delay);
            }
        }

        try!(self.link.poll());

        while let Async::Ready(Some(msg)) = try!(self.link_rx.poll()) {
            info!("received: {:?}", msg);
        }

        while let Async::Ready(Some(gossip_msg)) = try!(self.gossip.poll()) {
            info!("received gossip: {:?}", gossip_msg);
        }

        trace!("done polling app");

        Ok(Async::NotReady)
    }
}

pub fn load_node_ids(filename: &str) -> io::Result<HashSet<String>> {
    let mut result = HashSet::new();
    let f = try!(fs::File::open(filename));
    let mut f = io::BufReader::new(&f);
    for line in f.lines() {
        result.insert(try!(line));
    }
    Ok(result)
}

// args:
// [0]: program name
// [1]: listen address (== node id)
// [2]: file containing list of all node ids
pub fn main() {
    slog_envlogger::init().unwrap();

    let mut core = Core::new().unwrap();

    // This brings up our server.
    let node_id = std::env::args().nth(1).unwrap();
    let peers: HashSet<String> = load_node_ids(&std::env::args().nth(2).unwrap())
        .expect("error loading peer addresses")
        .into_iter()
        .filter(|i| *i != node_id)
        .collect();

    let addr: SocketAddr = node_id.parse().unwrap();
    let factory = NewLineClient::new(core.handle());

    info!("node_id={} {} peers listening on {}", node_id, peers.len(), addr);

    // gossip params
    let k = 3;
    let rounds = 2;
    let alpha = 0.5;
    let delta = Duration::milliseconds(200);

    let app = App::new(core.handle(), node_id, Duration::seconds(2), addr, factory, peers.clone(), k, rounds, alpha, delta).expect("error creating app");
    //app.add_conn("9000", addr).expect("error adding connection");
    for p in peers {
        app.add_conn(&p, p.parse().unwrap()).expect("error adding connection");
    }


    let client = core.run(app).expect("error running future");
}

use std::fmt;
use std::time::{Duration, SystemTime};
use std::sync::{Arc, Mutex};
use std::collections::{HashMap, HashSet};
use std::collections::hash_map;
use std::io;
use futures::{self, Async, Poll, Stream, Future};
use futures::sync::mpsc::{channel, Sender, Receiver};
use tokio_timer;
use tokio_core::reactor::Handle;
use tokio_core::net::TcpStream;
use serde_json::{self, Value};

use ::switchboard::{self, Switchboard};
use ::client::{NewClient};
use ::multiplex;
use ::{WatchdogMessage};

struct Message {
    pub to: String,
    pub key: String,
    pub body: WatchdogMessage,
}

#[derive(Clone)]
pub struct Heartbeat {
    pub client_id: String,
    pub time: SystemTime,
}

impl Heartbeat {
    pub fn new<S>(client_id: S, time: SystemTime) -> Heartbeat where S: Into<String> {
        Heartbeat {
            client_id: client_id.into(),
            time: time,
        }
    }

    pub fn max<'a>(&'a self, other: &'a Heartbeat) -> &'a Heartbeat {
        if self.time > other.time {
            self
        } else {
            other
        }
    }

    pub fn age(&self, now: SystemTime) -> io::Result<Duration> {
        now.duration_since(self.time)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, "systemtime duration_since"))
    }
}

pub struct Watchdog {
    id: String,
    handle: Handle,
    timer: tokio_timer::Timer,
    sleep: Option<tokio_timer::Sleep>,
    interval: Duration,
    max_age: Duration,
    link: Sender<multiplex::Command>,
    link_rx: Receiver<multiplex::Message>,
    multiplex_key: String,
    heartbeats: HashMap<String, Heartbeat>,
    failed: HashSet<String>,
    seq: u32,
}

impl Watchdog {

    pub fn new(handle: Handle, id: String, link: Sender<multiplex::Command>, interval: Duration, max_age: Duration) -> io::Result<Watchdog> {
        let timer = tokio_timer::Timer::default();
        let (link_tx, link_rx) = try!(channel(&handle));
        let multiplex_key = "watchdog";

        try!(link.send(multiplex::Command::Subscribe {key: multiplex_key.to_string(), sender: link_tx}));

        Ok(Watchdog {
            id: id,
            handle: handle,
            sleep: Some(timer.sleep(interval.clone())),
            timer: timer,
            interval: interval,
            max_age: max_age,
            link: link,
            link_rx: link_rx,
            multiplex_key: multiplex_key.to_string(),
            heartbeats: HashMap::new(),
            failed: HashSet::new(),
            seq: 0,
        })
    }

    fn handle(&mut self, msg: multiplex::Message) -> io::Result<()> {
        debug!("watchdog received {:?}", msg);

        let wm = try!(serde_json::from_value(msg.body)
            .map_err(|e| {
                io::Error::new(io::ErrorKind::Other, "json decoding error")
            }));

        match wm {
            WatchdogMessage::Ping { seq } => {
                try!(self.handle_ping(msg.from, seq));
            },
            WatchdogMessage::Pong { seq } => {
                try!(self.handle_pong(msg.from, seq));
            }
        }

        Ok(())
    }

    fn handle_ping(&mut self, id: String, seq: u32) -> io::Result<()> {
        let wm = WatchdogMessage::Pong { seq: seq };
        
        let body = serde_json::to_value(wm);

        let m = multiplex::Message::new(Some(id), self.id.clone(), self.multiplex_key.clone(), body);

        self.link.send(multiplex::Command::SendMessage {msg: m})
    }

    fn handle_pong(&mut self, id: String, seq: u32) -> io::Result<()> {
        let heartbeat = Heartbeat::new(id, SystemTime::now());

        try!(self.heartbeat(heartbeat));

        Ok(())
    }

    pub fn poll(&mut self) -> Poll<(), io::Error> {
        trace!("polling watchdog");

        try!(self.poll_timer());

        try!(self.check_failed());

        while let Async::Ready(Some(msg)) = try!(self.link_rx.poll()) {
            try!(self.handle(msg));
        }

        Ok(Async::NotReady)
    }

    pub fn poll_timer(&mut self) -> io::Result<()> {
        if let Some(mut sleep) = self.sleep.take() {
            trace!("checking timer");
            match try!(sleep.poll().map_err(|_te| io::Error::new(io::ErrorKind::Other, "timer error"))) {
                Async::Ready(()) => {
                    try!(self.send_pings());
                },
                Async::NotReady => {
                    self.sleep = Some(sleep);
                }
            }
        }
        
        if self.sleep.is_none() {
            self.sleep = Some(self.timer.sleep(self.interval.clone()));

            try!(self.poll_timer());
        }

        Ok(())
    }

    pub fn send_pings(&mut self) -> io::Result<()> {
        trace!("pinging peers");

        self.seq += 1;

        let body = serde_json::to_value(WatchdogMessage::Ping {seq: self.seq});

        let msg = multiplex::Message::new(None, self.id.clone(), self.multiplex_key.clone(), body);

        try!(self.link.send(multiplex::Command::SendMessage{ msg: msg}));

        Ok(())
    }

    pub fn heartbeat(&mut self, heartbeat: Heartbeat) -> io::Result<()> {
        match self.heartbeats.entry(heartbeat.client_id.clone()) {
            hash_map::Entry::Occupied(e) => {
                let h = e.into_mut();
                *h = h.max(&heartbeat).to_owned();
            },
            hash_map::Entry::Vacant(e) => {
                e.insert(heartbeat);
            }
        }

        Ok(())
    }

    pub fn check_failed(&mut self) -> io::Result<()> {
        let now = SystemTime::now();

        for h in self.heartbeats.values() {
            let age = try!(h.age(now));

            if age > self.max_age {
                if !self.failed.contains(&h.client_id) {
                    info!("marking peer {} as failed", h.client_id);
                    self.failed.insert(h.client_id.clone());
                }
            } else {
                if self.failed.contains(&h.client_id) {
                    info!("marking peer {} as recovered", h.client_id);
                    self.failed.remove(&h.client_id);
                }
            }
        }

        Ok(())
    }
}

impl Future for Watchdog {
    type Item=();
    type Error=io::Error;

    fn poll(&mut self) -> Poll<(), io::Error> {
        Watchdog::poll(self)
    }
}
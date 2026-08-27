//! Perfect failure detection.
//!
//! Cachin, Guerraoui & Rodrigues, Module 2.6 and Algorithm 2.5 ("Exclude on Timeout").
//!
//! **Status: deployable where synchrony is real. Space: bounded by membership.** All state is one
//! entry per peer. A system without a known delivery bound wants an eventually perfect detector
//! instead — see the timing note below.
//!
//! ```text
//! PFD1: Strong completeness: Eventually, every process that crashes is permanently
//!       detected by every correct process.
//! PFD2: Strong accuracy:     If a process p is detected by any process, then p has crashed.
//! ```
//!
//! **This protocol's guarantees are conditional on a timing assumption, and it is the first in
//! this repository that has one.** Every abstraction below is correct in an asynchronous model: it
//! assumes nothing about how long a message takes. Perfect detection is impossible there — a live
//! process whose messages are merely slow is indistinguishable from a dead one, so any detector
//! either accuses the living or never accuses the dead.
//!
//! What makes it possible is a *synchronous* system: message delivery between correct processes
//! within a known bound Δ. PFD2 holds only while that bound does. Run this detector on a lossy or
//! unbounded network and it will accuse correct processes — that is the assumption failing, not
//! the implementation.
//!
//! ```text
//! upon event ⟨ Timeout ⟩ do
//!     forall p ∈ Π do
//!         if p ∉ alive ∧ p ∉ detected then
//!             detected := detected ∪ {p};
//!             trigger ⟨ P, Crash | p ⟩;
//!     forall p ∈ Π do
//!         trigger ⟨ pl, Send | p, [HEARTBEATREQUEST] ⟩;
//!     alive := ∅;
//!     starttimer(Δ);
//! ```
//!
//! Four departures from the page:
//!
//! - The book exchanges a request and a reply each round. This sends one unsolicited heartbeat
//!   per round instead: with the same bound it distinguishes the same failures in half the
//!   messages, and the round-trip's only role was to let the requester choose when to ask.
//! - **The heartbeat period and the detection timeout are separate.** The book uses one delay Δ
//!   for both, which makes a single missed round fatal: a process stalled for an instant spanning
//!   its own send is accused, though it is alive and the network kept its promise. Beating every
//!   `period` and accusing only after `timeout` of silence tolerates a stall shorter than
//!   `timeout − period − Δ`. Accuracy still requires `timeout > period + Δ`.
//! - `⟨P, Init⟩` *is* the init event, and this protocol has no commands at all. The first timer is
//!   armed on the first tick request from the layer above.
//! - **Heartbeats go on the wire directly, not through `pl`.** Algorithm 2.5 sends over perfect
//!   links; this protocol is its own bottom layer and has no child. Under the synchronous
//!   configuration the two are equivalent — the sim's synchronous mode zeroes the loss knob and
//!   enforces the bound, so nothing is lost and a perfect link would add only deduplication that
//!   an idempotent `last_heard` write does not need. Outside it they differ, and in the direction
//!   that matters: a lost heartbeat is indistinguishable from a silent process, so loss forges
//!   the very evidence this protocol accuses on. `accuracy_is_lost_when_the_timing_assumption_is`
//!   is that difference, made to happen.
//!
//! # A stall has two sides, and the second one is worse
//!
//! The departure above covers the process a stall happens *to*: it misses a send and its peers
//! accuse it, which the `timeout − period − Δ` margin is what tolerates. The other side is the
//! stalled process itself, and it is not tolerated at all.
//!
//! A process descheduled for longer than `timeout` comes back holding a tick that is due and a
//! `last_heard` for every peer that is now older than the timeout. The measurement is not wrong —
//! it genuinely heard nothing for that long — so it accuses every one of them, and detection is
//! permanent. `a_stalled_process_accuses_its_peers_when_it_comes_back` pins that, and the pair of
//! suspension tests either side of it shows the margin still holds from the inside for a stall
//! shorter than the timeout.
//!
//! This is the timing assumption failing rather than the implementation, and it fails in the one
//! place the assumption is easiest to forget: Δ bounds the *network*, and a synchronous system
//! bounds process scheduling too. A detector that discounts its own stall — noticing that far
//! more than `period` elapsed between consecutive ticks, and treating that round as unmeasured —
//! is a real technique and a real departure from the page, so it is a change with a proposal
//! rather than something to add here. `Sim::resume` documents the same asymmetry from the
//! simulator's side.

use core::time::Duration;
use recon_core::{NodeId, ProtoCx, Protocol, Time, TimerId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// What a process says to show it is alive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heartbeat;

/// Requests from the layer above: none.
///
/// Detection begins at initialisation, as Module 2.6 has it, so there is nothing to ask for. An
/// uninhabited type says so in a way the compiler checks.
pub type Cmd = core::convert::Infallible;

/// Indications to the layer above.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ind {
    /// `node` has crashed. Raised exactly once per process, and never retracted.
    Crash { node: NodeId },
}

/// Detects crashes by heartbeat timeout.
#[derive(Debug)]
pub struct PerfectFailureDetector {
    me: NodeId,
    peers: BTreeSet<NodeId>,
    /// When each peer was last heard from.
    last_heard: BTreeMap<NodeId, Time>,
    /// Already reported. Detection is permanent, so these are never revisited.
    detected: BTreeSet<NodeId>,
    /// How often this process announces itself.
    period: Duration,
    /// How long a peer may be silent before it is declared crashed.
    timeout: Duration,
    /// The tick outstanding, if any. A handle rather than a flag: an expiry this detector has
    /// superseded is then recognisable, where `true` said only that something was pending.
    tick: Option<TimerId>,
}

impl PerfectFailureDetector {
    /// Detect among `peers`, announcing this process every `period` and declaring a peer crashed
    /// after `timeout` of silence.
    ///
    /// For strong accuracy, `timeout` must exceed `period` plus the network's delivery bound —
    /// otherwise a live process's heartbeat can arrive after it has already been accused.
    /// Configure the bound from [`recon_sim::Sim::delivery_bound`] rather than guessing it.
    pub fn new(
        me: NodeId,
        peers: impl IntoIterator<Item = NodeId>,
        period: Duration,
        timeout: Duration,
    ) -> Self {
        debug_assert!(
            timeout > period,
            "a timeout no longer than the heartbeat period accuses live processes"
        );
        let mut peers: BTreeSet<NodeId> = peers.into_iter().collect();
        peers.remove(&me);
        PerfectFailureDetector {
            me,
            peers,
            last_heard: BTreeMap::new(),
            detected: BTreeSet::new(),
            period,
            timeout,
            tick: None,
        }
    }

    /// How often this process announces itself.
    pub fn period(&self) -> Duration {
        self.period
    }

    /// The processes currently believed crashed.
    pub fn detected(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.detected.iter().copied()
    }

    /// Whether `node` has been detected as crashed.
    pub fn has_detected(&self, node: NodeId) -> bool {
        self.detected.contains(&node)
    }

    /// The processes not yet detected as crashed, this one included.
    pub fn correct(&self) -> impl Iterator<Item = NodeId> + '_ {
        core::iter::once(self.me)
            .chain(self.peers.iter().copied().filter(|p| !self.detected.contains(p)))
    }

    /// The silence a process is allowed before being declared crashed.
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    fn beat(&mut self, cx: &mut ProtoCx<'_, Self>) {
        for &p in &self.peers {
            cx.send(p, Heartbeat);
        }
    }

    /// Treat every peer as heard from now, so the first round does not accuse anyone before a
    /// heartbeat has had time to arrive.
    fn assume_alive(&mut self, now: Time) {
        for &p in &self.peers {
            self.last_heard.insert(p, now);
        }
    }
}

impl Protocol for PerfectFailureDetector {
    type Cmd = Cmd;
    type Ind = Ind;
    type Msg = Heartbeat;
    /// No scope conditions: this protocol's guarantees do not lapse.
    type Scope = core::convert::Infallible;
    /// Keeps nothing durably: a crash loses everything this protocol knows.
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    /// `⟨ P, Init ⟩ do alive := Π; detected := ∅; starttimer(Δ)`.
    ///
    /// The book's own trigger, now that there is one. It used to be a `Start` command because
    /// there was no init event to hang the first timer on; there is, so there is no command.
    fn on_init(&mut self, cx: &mut ProtoCx<'_, Self>) {
        if self.tick.is_some() {
            return;
        }
        self.assume_alive(cx.now());
        self.beat(cx);
        self.tick = Some(cx.set_timer(self.period));
    }

    fn on_cmd(&mut self, cmd: Cmd, _cx: &mut ProtoCx<'_, Self>) {
        match cmd {}
    }

    fn on_msg(&mut self, from: NodeId, Heartbeat: Heartbeat, cx: &mut ProtoCx<'_, Self>) {
        // A process already detected stays detected: PFD1 demands permanence, and under the
        // timing assumption a heartbeat from an accused process cannot arrive.
        if self.peers.contains(&from) {
            self.last_heard.insert(from, cx.now());
        }
    }

    fn on_timer(&mut self, id: TimerId, cx: &mut ProtoCx<'_, Self>) {
        // Handed down from every layer above along with everybody else's, so only the one this
        // detector is waiting on does anything. An expiry it has superseded accuses nobody.
        if self.tick != Some(id) {
            return;
        }
        let now = cx.now();
        let silent: Vec<NodeId> = self
            .peers
            .iter()
            .copied()
            .filter(|p| !self.detected.contains(p))
            .filter(|p| {
                let last = self.last_heard.get(p).copied().unwrap_or(Time::ZERO);
                now.saturating_since(last) > self.timeout
            })
            .collect();
        for p in silent {
            self.detected.insert(p);
            cx.indicate(Ind::Crash { node: p });
        }

        self.beat(cx);
        self.tick = Some(cx.set_timer(self.period));
    }
}

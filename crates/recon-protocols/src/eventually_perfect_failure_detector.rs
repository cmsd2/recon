//! Eventually perfect failure detection — ◇P.
//!
//! **Status: implementation. Space: bounded by membership.** One entry per peer and nothing per
//! message, as the perfect detector has, plus one adaptive delay.
//!
//! Cachin, Guerraoui & Rodrigues, Module 2.8 and Algorithm 2.7 ("Increasing Timeout"), quoted from
//! the book:
//!
//! ```text
//! Algorithm 2.7: Increasing Timeout
//! Implements: EventuallyPerfectFailureDetector, instance ◇P.
//! Uses: PerfectPointToPointLinks, instance pl.
//!
//! upon event ⟨ ◇P, Init ⟩ do
//!     alive := Π; suspected := ∅; delay := Δ;
//!     starttimer(delay);
//!
//! upon event ⟨ Timeout ⟩ do
//!     if alive ∩ suspected ≠ ∅ then
//!         delay := delay + Δ;
//!     forall p ∈ Π do
//!         if (p ∉ alive) ∧ (p ∉ suspected) then
//!             suspected := suspected ∪ {p};
//!             trigger ⟨ ◇P, Suspect | p ⟩;
//!         else if (p ∈ alive) ∧ (p ∈ suspected) then
//!             suspected := suspected \ {p};
//!             trigger ⟨ ◇P, Restore | p ⟩;
//!         trigger ⟨ pl, Send | p, [HEARTBEATREQUEST] ⟩;
//!     alive := ∅;
//!     starttimer(delay);
//! ```
//!
//! # What ◇P is for, and why this repository now needs it
//!
//! [`crate::perfect_failure_detector`] promises *if a process is detected, it has crashed* — never
//! wrong, never retracted — and is implementable only where a delivery bound Δ is **known in
//! advance**. ◇P weakens that to *eventually, no correct process is suspected*, and therefore must
//! be able to take a suspicion back. `Restore` is the whole difference.
//!
//! It matters here because a crashed process now **comes back**. Under `P` a suspicion is permanent
//! by construction, so Ω's `suspected` set only grows, `maxrank` only ever walks downward through
//! the membership, and a recovered process can never lead again.
//!
//! # `alive ∩ suspected ≠ ∅` is the algorithm noticing it was wrong
//!
//! The guard reads: *someone I suspected has been heard from this round*. That is a false suspicion
//! caught in the act, and the book's response is to wait longer next time. Read the negations
//! carefully — an OCR of the page drops them from `if (p ∉ alive) ∧ (p ∉ suspected)`, and taking it
//! as written inverts the algorithm into one that suspects whoever it just heard from.
//!
//! # Departure: the delay comes down again
//!
//! Algorithm 2.7 adds `Δ` on every false suspicion and **never subtracts**. That is a ratchet, and
//! its cost is not the unboundedness but the irreversibility: one bad period leaves detection
//! permanently slower for the rest of the run, long after the network recovered, with nothing
//! reporting that it has.
//!
//! So after [`Config::quiet_rounds`] consecutive rounds in which **nothing at all was suspected**,
//! the delay comes down by one `step` — never below [`Config::min_delay`]. **Down slowly, up fast**:
//! the increase is immediate and the decrease waits, and that asymmetry is what damps the
//! oscillation a symmetric rule would produce around the true bound.
//!
//! **"Nothing suspected", not "nothing withdrawn", and the difference is the whole rule.** The first
//! draft eased off after rounds in which no suspicion was *taken back*, which is wrong in the worst
//! case: a network bad enough that suspicions are never withdrawn produces no withdrawals at all, so
//! the delay came down while the detector was being consistently wrong. Measured: against a network
//! twelve times the initial delay, the delay drifted back to the floor instead of pinning at the
//! cap. With nothing suspected there is no outstanding claim that could be wrong and the network is
//! evidently keeping up, which is the only situation in which easing off is defensible.
//!
//! The price is that a genuinely crashed peer, permanently suspected, **freezes** the delay wherever
//! it had reached. That is deliberate: with a crashed process in the membership there is no clean
//! signal to ease off on, and freezing is strictly better than the ratchet — which grows — and than
//! decreasing blindly, which gets more wrong. A detector that could tell the two apart would be
//! measuring the observed silence of the peers it is *not* suspecting, which is an accrual detector,
//! and that is the next change rather than this one.
//!
//! # What drives the increase, which is not what you would guess
//!
//! `alive ∩ suspected ≠ ∅` fires when a suspected process is heard from — a false suspicion caught
//! **in the act of being corrected**. So the delay climbs with the rate at which the detector is
//! *caught out*, not the rate at which it is wrong. A detector that is consistently wrong, because
//! every peer is beyond the delay every round, is never corrected and never climbs. That is
//! Algorithm 2.7's own behaviour rather than anything added here, and it is why the increase alone
//! does not converge on a bad network — the cap is what makes the failure bounded and stated.
//!
//! What it costs: *strict* eventual accuracy under partial synchrony, where the delay bound is
//! merely finite and unknown. Under a bound that never settles, a detector that can come down can be
//! wrong for ever. What it buys back is accuracy under a weaker and more realistic assumption — that
//! the true delay eventually stops **changing** — under which the estimate converges and the ratchet
//! does not.
//!
//! # Departure: the delay is capped
//!
//! `delay` never exceeds [`Config::max_delay`]. Unconditional eventual accuracy needs unbounded
//! growth, because partial synchrony refuses to let you assume any bound in advance — but that is a
//! property of the model rather than of networks, and an operator knows their delay distribution to
//! within orders of magnitude.
//!
//! **Why capping is the right loss.** Ask what a wrong ◇P breaks. Ω trusts the wrong leader, which
//! costs an epoch change and an abort — liveness, never safety; `leader_driven_consensus` states
//! agreement as `[always]` and termination as already conditional on the detector settling. So a cap
//! that is occasionally too small costs progress during a network episode and clears when the
//! episode does. The uncapped ratchet costs progress *permanently*, and silently. Both are liveness
//! failures; only one recovers.
//!
//! Exceeding the cap is the stated condition failing rather than the implementation, exactly as Δ
//! is for the perfect detector — and `tests/eventually_perfect_failure_detector.rs` makes it happen
//! rather than describing it.
//!
//! # Departure: heartbeats are unsolicited, and the period is not the timeout
//!
//! Both inherited from [`crate::perfect_failure_detector`], for the reasons its documentation gives:
//! one unsolicited beat per round distinguishes the same failures in half the messages of a
//! request/reply exchange, and separating the beat period from the silence a peer is allowed stops a
//! single missed round being fatal. Here the *timeout* is what adapts; the period does not.
//!
//! ```text
//! ◇P1 [always]      Strong completeness — every crashed process is eventually permanently
//!                   suspected by every correct process
//! ◇P2 [Δ ≤ max_delay ∧ Δ eventually stable]
//!                   Eventual strong accuracy — eventually no correct process is suspected
//! ```
//!
//! `◇P2`'s scope is the two departures above, written down. The same shape as `PB2 [window]` and
//! `SL1 [session]`: the guarantee is conditional and the condition is named, rather than the
//! guarantee being quietly weaker than the page's.

use core::time::Duration;
use recon_core::{NodeId, ProtoCx, Protocol, Time, TimerId};
use std::collections::{BTreeMap, BTreeSet};

use crate::detector::{Detector, DetectorInd};
use crate::perfect_failure_detector::Heartbeat;

/// Requests from the layer above: none, as Module 2.8 has it.
pub type Cmd = core::convert::Infallible;

/// Indications to the layer above.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ind {
    /// `⟨ ◇P, Suspect | p ⟩` — `node` is suspected of having crashed. May be wrong, and may be
    /// followed by [`Ind::Restore`].
    Suspect { node: NodeId },
    /// `⟨ ◇P, Restore | p ⟩` — `node` is no longer suspected. The indication `P` does not have.
    Restore { node: NodeId },
}

/// How this detector beats, waits and adapts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    /// How often this process announces itself. Fixed; only the timeout adapts.
    pub period: Duration,
    /// The silence a peer is allowed before it is suspected, to begin with. The book's `Δ`.
    pub initial_delay: Duration,
    /// The book's `Δ` again, as the amount the delay moves by in either direction.
    pub step: Duration,
    /// The delay never falls below this.
    pub min_delay: Duration,
    /// The delay never rises above this. See the module note on what the cap costs.
    pub max_delay: Duration,
    /// How many consecutive rounds with **nothing suspected** before the delay comes down. Up
    /// fast, down slow: this is the asymmetry that damps the oscillation. See the module note on
    /// why the condition is "nothing suspected" rather than "nothing withdrawn".
    pub quiet_rounds: u32,
}

impl Config {
    /// A configuration that beats every `period` and starts by allowing `initial_delay` of silence,
    /// stepping by the period, never below it and never above `max_delay`, easing off after four
    /// quiet rounds.
    pub fn new(period: Duration, initial_delay: Duration, max_delay: Duration) -> Self {
        Config {
            period,
            initial_delay,
            step: period,
            min_delay: initial_delay,
            max_delay,
            quiet_rounds: 4,
        }
    }
}

/// Detects crashes by heartbeat timeout, and changes its mind.
#[derive(Debug)]
pub struct EventuallyPerfectFailureDetector {
    me: NodeId,
    peers: BTreeSet<NodeId>,
    /// When each peer was last heard from. `alive` in the book is *this round's* arrivals; a
    /// timestamp says the same thing and survives the period and timeout being separate.
    last_heard: BTreeMap<NodeId, Time>,
    /// `suspected`. Grows and shrinks, which is the point.
    suspected: BTreeSet<NodeId>,
    config: Config,
    /// `delay`.
    delay: Duration,
    /// Consecutive rounds with nothing suspected at all.
    quiet: u32,
    /// The tick outstanding. A handle, so an expiry this detector has superseded accuses nobody.
    tick: Option<TimerId>,
}

impl EventuallyPerfectFailureDetector {
    /// Detect among `peers`, adapting as `config` says.
    pub fn new(me: NodeId, peers: impl IntoIterator<Item = NodeId>, config: Config) -> Self {
        debug_assert!(
            config.initial_delay > config.period,
            "a delay no longer than the heartbeat period suspects live processes every round"
        );
        debug_assert!(config.min_delay <= config.max_delay, "an empty range for the delay");
        let mut peers: BTreeSet<NodeId> = peers.into_iter().collect();
        peers.remove(&me);
        EventuallyPerfectFailureDetector {
            me,
            peers,
            last_heard: BTreeMap::new(),
            suspected: BTreeSet::new(),
            config,
            delay: config.initial_delay,
            quiet: 0,
            tick: None,
        }
    }

    /// The processes currently suspected. May shrink.
    pub fn suspected(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.suspected.iter().copied()
    }

    /// Whether `node` is currently suspected.
    pub fn suspects(&self, node: NodeId) -> bool {
        self.suspected.contains(&node)
    }

    /// The processes not currently suspected, this one included.
    pub fn correct(&self) -> impl Iterator<Item = NodeId> + '_ {
        core::iter::once(self.me)
            .chain(self.peers.iter().copied().filter(|p| !self.suspected.contains(p)))
    }

    /// The silence a peer is currently allowed. Adapts; see the module's two departures.
    pub fn delay(&self) -> Duration {
        self.delay
    }

    /// How often this process announces itself.
    pub fn period(&self) -> Duration {
        self.config.period
    }

    fn beat(&mut self, cx: &mut ProtoCx<'_, Self>) {
        for &p in &self.peers {
            cx.send(p, Heartbeat);
        }
    }
}

impl Detector for EventuallyPerfectFailureDetector {
    fn classify(ind: Ind) -> DetectorInd {
        match ind {
            Ind::Suspect { node } => DetectorInd::Suspect { node },
            Ind::Restore { node } => DetectorInd::Restore { node },
        }
    }
}

impl Protocol for EventuallyPerfectFailureDetector {
    type Cmd = Cmd;
    type Ind = Ind;
    type Msg = Heartbeat;
    /// No scope conditions of its own: `◇P2`'s conditions are on the network, not on a scope this
    /// protocol is told about.
    type Scope = core::convert::Infallible;
    /// Keeps nothing durably. A restarted detector suspects nobody and learns again.
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    /// `⟨ ◇P, Init ⟩ do alive := Π; suspected := ∅; delay := Δ; starttimer(delay)`.
    fn on_init(&mut self, cx: &mut ProtoCx<'_, Self>) {
        if self.tick.is_some() {
            return;
        }
        let now = cx.now();
        for &p in &self.peers {
            self.last_heard.insert(p, now);
        }
        self.beat(cx);
        self.tick = Some(cx.set_timer(self.config.period));
    }

    fn on_cmd(&mut self, cmd: Cmd, _cx: &mut ProtoCx<'_, Self>) {
        match cmd {}
    }

    /// A heartbeat is `alive := alive ∪ {p}`. Note what is *not* here: no check against
    /// `suspected`. Hearing from a suspected process is exactly the case `Restore` exists for, and
    /// the round's own pass is where it is noticed.
    fn on_msg(&mut self, from: NodeId, Heartbeat: Heartbeat, cx: &mut ProtoCx<'_, Self>) {
        if self.peers.contains(&from) {
            self.last_heard.insert(from, cx.now());
        }
    }

    fn on_timer(&mut self, id: TimerId, cx: &mut ProtoCx<'_, Self>) {
        if self.tick != Some(id) {
            return;
        }
        let now = cx.now();
        let heard_from = |d: &Self, p: NodeId| {
            let last = d.last_heard.get(&p).copied().unwrap_or(Time::ZERO);
            now.saturating_since(last) <= d.delay
        };

        // `if alive ∩ suspected ≠ ∅ then delay := delay + Δ` — a suspicion caught being corrected.
        let was_wrong =
            self.peers.iter().any(|p| self.suspected.contains(p) && heard_from(self, *p));
        // Departure: down one step after `quiet_rounds` rounds with nothing suspected, never below
        // the floor. Anything outstanding — right or wrong — holds the delay where it is.
        let nothing_suspected =
            self.suspected.is_empty() && self.peers.iter().all(|p| heard_from(self, *p));
        if was_wrong {
            self.delay = (self.delay + self.config.step).min(self.config.max_delay);
            self.quiet = 0;
        } else if nothing_suspected {
            self.quiet += 1;
            if self.quiet >= self.config.quiet_rounds {
                self.quiet = 0;
                self.delay = self.delay.saturating_sub(self.config.step).max(self.config.min_delay);
            }
        } else {
            self.quiet = 0;
        }

        // `forall p ∈ Π do if (p ∉ alive) ∧ (p ∉ suspected) … else if (p ∈ alive) ∧ (p ∈ suspected)`
        let changes: Vec<Ind> = self
            .peers
            .iter()
            .copied()
            .filter_map(|p| match (heard_from(self, p), self.suspected.contains(&p)) {
                (false, false) => Some(Ind::Suspect { node: p }),
                (true, true) => Some(Ind::Restore { node: p }),
                _ => None,
            })
            .collect();
        for change in changes {
            match change {
                Ind::Suspect { node } => {
                    self.suspected.insert(node);
                }
                Ind::Restore { node } => {
                    self.suspected.remove(&node);
                }
            }
            cx.indicate(change);
        }

        self.beat(cx);
        self.tick = Some(cx.set_timer(self.config.period));
    }
}

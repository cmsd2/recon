//! What a run does to the messages passing through it.

use core::time::Duration;

/// Network conditions and run limits.
///
/// Every knob is consulted through the run's seeded generator, so a configuration plus a seed
/// determines a run completely.
#[derive(Debug, Clone)]
pub struct Config {
    /// Seed for every random decision in the run — faults, latency, and protocol randomness.
    pub seed: u64,
    /// Probability in `0.0..=1.0` that a message is dropped rather than delivered.
    pub loss: f64,
    /// Probability in `0.0..=1.0` that a message is delivered twice.
    pub duplication: f64,
    /// Probability in `0.0..=1.0` that a message is delayed far beyond normal latency,
    /// forcing it behind messages sent after it.
    pub reorder: f64,
    /// Shortest delivery delay.
    pub latency_min: Duration,
    /// Longest delivery delay. Jitter between the two produces ordinary reordering.
    pub latency_max: Duration,
    /// Extra delay applied to a message selected for reordering.
    pub reorder_delay: Duration,
    /// How long a write to stable storage takes to become durable.
    ///
    /// Not zero, and deliberately: a crash that lands while a write is outstanding is the fault
    /// that finds bugs in anything written for the fail-recovery model, and it cannot happen at
    /// all if writes complete instantaneously.
    pub write_latency: Duration,
    /// When set, the run is synchronous: delivery between connected, uncrashed processes is
    /// guaranteed within this bound, and nothing is lost, duplicated or given a reordering spike.
    ///
    /// This is what a perfect failure detector needs and what the asynchronous default cannot
    /// offer: without a known bound, a live process whose messages are unlucky is
    /// indistinguishable from a crashed one. It constrains *timing* only — crashes and
    /// partitions still stop delivery.
    pub synchronous: Option<Duration>,
    /// When true, communication happens within sessions: between each pair of processes there is
    /// a session in which delivery is reliable, ordered and free of duplicates — what TCP or QUIC
    /// gives. A partition, a crash, or an explicit break ends it, losing an unknown suffix of what
    /// was in flight, and a new session begins at a higher epoch.
    ///
    /// This is the model a deployed stack would run on. The fair-loss default is what you have if
    /// you build reliability yourself, which is the simulator's own situation and not
    /// production's.
    pub sessions: bool,
    /// How often a session-based run retries establishing sessions that are not up.
    ///
    /// A deployed link keeps trying to reconnect on its own rather than waiting for the layers
    /// above to transmit, so the model does too. The value stands in for a retry interval, with or
    /// without backoff; no protocol may depend on it.
    pub reconnect_interval: Duration,
    /// Safety valve: a run stops after this many events, whatever the clock says.
    ///
    /// Protocols such as the stubborn link retransmit forever by design, so a run is bounded
    /// by time or by this, never by quiescence.
    pub max_steps: u64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            seed: 0,
            loss: 0.0,
            duplication: 0.0,
            reorder: 0.0,
            latency_min: Duration::from_millis(1),
            latency_max: Duration::from_millis(1),
            reorder_delay: Duration::from_millis(50),
            write_latency: Duration::from_millis(1),
            synchronous: None,
            sessions: false,
            reconnect_interval: Duration::from_millis(5),
            max_steps: 1_000_000,
        }
    }
}

impl Config {
    /// How long a write to stable storage takes to become durable.
    pub fn write_latency(mut self, d: Duration) -> Self {
        self.write_latency = d;
        self
    }

    pub fn seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    /// Drop messages with probability `p`.
    pub fn loss(mut self, p: f64) -> Self {
        self.loss = p;
        self
    }

    /// Deliver messages twice with probability `p`.
    pub fn duplication(mut self, p: f64) -> Self {
        self.duplication = p;
        self
    }

    /// Delay messages far beyond normal latency with probability `p`, forcing reordering.
    pub fn reorder(mut self, p: f64) -> Self {
        self.reorder = p;
        self
    }

    /// Deliver after a delay drawn uniformly from `min..=max`.
    ///
    /// Jitter here is itself a source of reordering; `reorder` forces the extreme case.
    pub fn latency(mut self, min: Duration, max: Duration) -> Self {
        self.latency_min = min;
        self.latency_max = max;
        self
    }

    /// Run synchronously: every message between connected, uncrashed processes is delivered
    /// within `bound`, and none is lost or duplicated.
    ///
    /// The bound is readable afterwards through [`Config::delivery_bound`], so a protocol whose
    /// correctness depends on it can be configured from the same value rather than from a guess.
    /// Setting this overrides the fault knobs, and the override is enforced at delivery time —
    /// calling `loss` afterwards will not quietly reintroduce loss.
    pub fn synchronous(mut self, bound: Duration) -> Self {
        self.synchronous = Some(bound);
        self.loss = 0.0;
        self.duplication = 0.0;
        self.reorder = 0.0;
        self.latency_max = bound;
        if self.latency_min > bound {
            self.latency_min = bound;
        }
        self
    }

    /// The upper bound on delivery, when the run is synchronous.
    pub fn delivery_bound(&self) -> Option<Duration> {
        self.synchronous
    }

    /// Whether this run makes a timing guarantee.
    pub fn is_synchronous(&self) -> bool {
        self.synchronous.is_some()
    }

    /// Communicate within sessions: reliable, ordered, duplicate-free delivery while a session
    /// holds, and an unknown lost suffix when one ends.
    ///
    /// Overrides the loss and duplication knobs, enforced at delivery time so builder order
    /// cannot reintroduce them. Latency still applies, but a message is never delivered before one
    /// sent earlier to the same peer.
    pub fn sessions(mut self) -> Self {
        self.sessions = true;
        self.loss = 0.0;
        self.duplication = 0.0;
        self.reorder = 0.0;
        self
    }

    /// How often to retry establishing sessions that are not up.
    pub fn reconnect_interval(mut self, d: Duration) -> Self {
        self.reconnect_interval = d;
        self
    }

    /// Whether this run communicates within sessions.
    pub fn is_session_based(&self) -> bool {
        self.sessions
    }

    pub fn max_steps(mut self, n: u64) -> Self {
        self.max_steps = n;
        self
    }
}

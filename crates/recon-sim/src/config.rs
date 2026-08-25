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
            max_steps: 1_000_000,
        }
    }
}

impl Config {
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

    pub fn max_steps(mut self, n: u64) -> Self {
        self.max_steps = n;
        self
    }
}

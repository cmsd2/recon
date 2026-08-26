//! The record of what a run actually did.
//!
//! Properties are asserted over this rather than over protocol internals: a trace says what
//! was sent, what arrived, what was lost, and what each protocol claimed to deliver, which is
//! exactly the vocabulary the guarantees are written in.

use recon_core::{NodeId, Time, WriteKind};

/// Why a message never arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropReason {
    /// The network lost it — ordinary fair-loss behaviour.
    Lost,
    /// Sender and recipient were in different partitions.
    Partitioned,
    /// The recipient had crashed.
    RecipientCrashed,
    /// The session carrying it ended before it arrived.
    SessionEnded,
    /// The sender had crashed before the message left.
    SenderCrashed,
}

/// One thing that happened, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TraceEvent<M, I, T> {
    /// A protocol asked for a message to be transmitted.
    Sent { at: Time, from: NodeId, to: NodeId, msg: M },
    /// A message was handed to the recipient protocol.
    Delivered { at: Time, from: NodeId, to: NodeId, msg: M },
    /// A message was not delivered.
    Dropped { at: Time, from: NodeId, to: NodeId, msg: M, reason: DropReason },
    /// The network scheduled a second copy of a message.
    Duplicated { at: Time, from: NodeId, to: NodeId, msg: M },
    /// A message was selected for extreme delay.
    Reordered { at: Time, from: NodeId, to: NodeId, msg: M },
    /// A timer previously set by a protocol fired.
    TimerFired { at: Time, node: NodeId, token: T },
    /// A protocol delivered on its guarantee to the layer above.
    Indicated { at: Time, node: NodeId, ind: I },
    /// A session was established between two processes.
    SessionOpened { at: Time, a: NodeId, b: NodeId, epoch: u64 },
    /// A session ended. Anything still in flight may have been discarded.
    SessionEnded { at: Time, a: NodeId, b: NodeId, epoch: u64, reason: DropReason },
    /// A message discarded because the session carrying it ended.
    SuffixLost { at: Time, from: NodeId, to: NodeId, msg: M },
    /// A process crashed, losing its volatile state.
    Crashed { at: Time, node: NodeId },
    /// A process was suspended, keeping its state.
    Suspended { at: Time, node: NodeId },
    /// A process restarted.
    Restarted { at: Time, node: NodeId },
    /// A durable write. `kind` distinguishes rewriting metadata from appending, so a claim about
    /// a protocol's write cost can be checked rather than asserted.
    Wrote { at: Time, node: NodeId, kind: WriteKind },
    /// A process died inside a write. Whether that write landed is decided by the seed and is
    /// deliberately not recorded: the point of the fault is that nobody knows until the recovered
    /// process reads its storage back.
    DiedWriting { at: Time, node: NodeId },
    /// A restarted process was given back what it had written. `had_state` is false when it had
    /// written nothing and started as if for the first time.
    Recovered { at: Time, node: NodeId, had_state: bool },
}

impl<M, I, T> TraceEvent<M, I, T> {
    pub fn at(&self) -> Time {
        match self {
            TraceEvent::Sent { at, .. }
            | TraceEvent::Delivered { at, .. }
            | TraceEvent::Dropped { at, .. }
            | TraceEvent::Duplicated { at, .. }
            | TraceEvent::Reordered { at, .. }
            | TraceEvent::TimerFired { at, .. }
            | TraceEvent::Indicated { at, .. }
            | TraceEvent::SessionOpened { at, .. }
            | TraceEvent::SessionEnded { at, .. }
            | TraceEvent::SuffixLost { at, .. }
            | TraceEvent::Crashed { at, .. }
            | TraceEvent::Suspended { at, .. }
            | TraceEvent::Restarted { at, .. }
            | TraceEvent::Wrote { at, .. }
            | TraceEvent::DiedWriting { at, .. }
            | TraceEvent::Recovered { at, .. } => *at,
        }
    }
}

/// An ordered log of everything a run did.
#[derive(Debug, Clone)]
pub struct Trace<M, I, T> {
    events: Vec<TraceEvent<M, I, T>>,
}

impl<M, I, T> Default for Trace<M, I, T> {
    fn default() -> Self {
        Trace { events: Vec::new() }
    }
}

impl<M, I, T> Trace<M, I, T> {
    pub(crate) fn push(&mut self, e: TraceEvent<M, I, T>) {
        self.events.push(e);
    }

    pub fn events(&self) -> &[TraceEvent<M, I, T>] {
        &self.events
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Every indication raised, in order, with the process that raised it.
    pub fn indications(&self) -> impl Iterator<Item = (NodeId, &I)> {
        self.events.iter().filter_map(|e| match e {
            TraceEvent::Indicated { node, ind, .. } => Some((*node, ind)),
            _ => None,
        })
    }

    /// Every indication raised by `node`, in order.
    pub fn indications_at(&self, node: NodeId) -> impl Iterator<Item = &I> {
        self.indications().filter(move |(n, _)| *n == node).map(|(_, i)| i)
    }

    /// Every message actually handed to a recipient.
    pub fn deliveries(&self) -> impl Iterator<Item = (NodeId, NodeId, &M)> {
        self.events.iter().filter_map(|e| match e {
            TraceEvent::Delivered { from, to, msg, .. } => Some((*from, *to, msg)),
            _ => None,
        })
    }

    /// Every message a protocol asked to transmit.
    pub fn sends(&self) -> impl Iterator<Item = (NodeId, NodeId, &M)> {
        self.events.iter().filter_map(|e| match e {
            TraceEvent::Sent { from, to, msg, .. } => Some((*from, *to, msg)),
            _ => None,
        })
    }

    /// How many messages were dropped, for any reason.
    pub fn drops(&self) -> usize {
        self.events.iter().filter(|e| matches!(e, TraceEvent::Dropped { .. })).count()
    }

    /// How many messages were dropped for a specific reason.
    pub fn drops_because(&self, reason: DropReason) -> usize {
        self.events
            .iter()
            .filter(|e| matches!(e, TraceEvent::Dropped { reason: r, .. } if *r == reason))
            .count()
    }

    pub fn duplicates(&self) -> usize {
        self.events.iter().filter(|e| matches!(e, TraceEvent::Duplicated { .. })).count()
    }

    pub fn reorderings(&self) -> usize {
        self.events.iter().filter(|e| matches!(e, TraceEvent::Reordered { .. })).count()
    }

    /// How many sessions ended during the run.
    pub fn session_ends(&self) -> usize {
        self.events.iter().filter(|e| matches!(e, TraceEvent::SessionEnded { .. })).count()
    }

    /// How many messages were discarded because a session carrying them ended.
    pub fn suffix_losses(&self) -> usize {
        self.events.iter().filter(|e| matches!(e, TraceEvent::SuffixLost { .. })).count()
    }

    /// The epochs at which sessions were established, in order.
    pub fn session_epochs(&self) -> impl Iterator<Item = (NodeId, NodeId, u64)> + '_ {
        self.events.iter().filter_map(|e| match e {
            TraceEvent::SessionOpened { a, b, epoch, .. } => Some((*a, *b, *epoch)),
            _ => None,
        })
    }

    /// How many entries were appended.
    pub fn appends(&self) -> usize {
        self.events
            .iter()
            .filter(|e| matches!(e, TraceEvent::Wrote { kind: WriteKind::Append, .. }))
            .count()
    }

    /// How many times the metadata was replaced.
    pub fn metadata_writes(&self) -> usize {
        self.events
            .iter()
            .filter(|e| matches!(e, TraceEvent::Wrote { kind: WriteKind::Set, .. }))
            .count()
    }

    /// How many writes happened, of either kind.
    pub fn writes(&self) -> usize {
        self.events.iter().filter(|e| matches!(e, TraceEvent::Wrote { .. })).count()
    }

    /// How many times a process died inside a write.
    ///
    /// Not how many writes were *lost*: the seed decides that, and the trace does not say, which
    /// is the whole content of the fault.
    pub fn deaths_in_writes(&self) -> usize {
        self.events.iter().filter(|e| matches!(e, TraceEvent::DiedWriting { .. })).count()
    }

    /// How many restarts recovered durable state, as opposed to starting afresh.
    pub fn recoveries_with_state(&self) -> usize {
        self.events
            .iter()
            .filter(|e| matches!(e, TraceEvent::Recovered { had_state: true, .. }))
            .count()
    }

    pub fn timer_fires(&self) -> usize {
        self.events.iter().filter(|e| matches!(e, TraceEvent::TimerFired { .. })).count()
    }

    pub fn delivery_count(&self) -> usize {
        self.events.iter().filter(|e| matches!(e, TraceEvent::Delivered { .. })).count()
    }

    pub fn send_count(&self) -> usize {
        self.events.iter().filter(|e| matches!(e, TraceEvent::Sent { .. })).count()
    }

    pub fn indication_count(&self) -> usize {
        self.events.iter().filter(|e| matches!(e, TraceEvent::Indicated { .. })).count()
    }
}

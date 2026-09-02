//! The record of what a run actually did.
//!
//! Properties are asserted over this rather than over protocol internals: a trace says what
//! was sent, what arrived, what was lost, and what each protocol claimed to deliver, which is
//! exactly the vocabulary the guarantees are written in.

use recon_core::{NodeId, Protocol, Time, TimerId, WriteKind};

/// Names one operation given to a process, so a caller can find in the trace the thing it just
/// asked for.
///
/// Minted by the run, like [`recon_core::TimerId`], and for the same reason: one source per run
/// means two operations cannot share an identity. Unlike a timer handle it never reaches a
/// protocol — commands are unchanged, and nothing above the simulator sees one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OpId(pub u64);

impl core::fmt::Display for OpId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "op{}", self.0)
    }
}

/// Why an operation never reached the process it was given to.
///
/// Carried rather than flattened away, because the next thing built on this has to tell these
/// apart: an operation refused by a stall certainly did not happen, where one lost to a crash may
/// have been half-done by the incarnation that died.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotBegun {
    /// The process had crashed and not restarted. Its volatile state went with it.
    Crashed,
    /// The process was stalled. A command is a call from the layer above, on that process, so a
    /// stalled process's layer above is stalled with it — there was nothing to make the call.
    Stalled,
    /// There is no such process in this run.
    NotAProcess,
}

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
    /// Sent in the instant a session ended, before its successor could open. A transport does
    /// not reopen a connection in the instant it closed one; the next instant's send does.
    NoSession,
    /// The sender had crashed before the message left.
    SenderCrashed,
}

/// One thing that happened, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TraceEvent<M, I, N, C> {
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
    TimerFired { at: Time, node: NodeId, id: TimerId },
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
    /// A suspended process resumed, and everything held for it was dispatched.
    Resumed { at: Time, node: NodeId },
    /// A crashed process restarted, and took its startup branch.
    Restarted { at: Time, node: NodeId },
    /// A durable write. `kind` distinguishes rewriting metadata from appending, so a claim about
    /// a protocol's write cost can be checked rather than asserted.
    Wrote { at: Time, node: NodeId, kind: WriteKind },
    /// A process died inside a write. Whether that write landed is decided by the seed and is
    /// deliberately not recorded: the point of the fault is that nobody knows until the recovered
    /// process reads its storage back.
    DiedWriting { at: Time, node: NodeId },
    /// A process was given an operation, and handled it.
    ///
    /// Recorded when the handler ran, not when the command was scheduled. A handler's effects
    /// cannot precede the handler, so this is a valid left-hand end of the interval containing the
    /// operation's effect, and a tighter one than the moment the caller asked — which matters,
    /// because a suite that schedules several commands at one instant would otherwise show them all
    /// overlapping each other.
    Invoked { at: Time, node: NodeId, op: OpId, cmd: C },
    /// A process was given an operation and never handled it.
    ///
    /// Recorded rather than discarded silently: an operation asked for and never begun is not the
    /// same as one never asked for, and a record that cannot tell them apart is one a checker would
    /// reason from falsely.
    NotInvoked { at: Time, node: NodeId, op: OpId, cmd: C, why: NotBegun },
    /// A process narrated a decision it took.
    ///
    /// The one event here that is not something that *happened to* a process. It is in the same
    /// account, on the same clock, precisely so that a claim can be read against the run — a
    /// process saying it refused an announcement is a process from which no acceptance followed,
    /// and a test can require that rather than trust it.
    ///
    /// Recorded **before** the writes and effects of the handler that narrated it: a note marks the
    /// decision, and the write and the sends are what the decision led to.
    Said { at: Time, node: NodeId, note: N },
    /// A restarted process was given back what it had written. `had_state` is false when it had
    /// written nothing and started as if for the first time.
    Recovered { at: Time, node: NodeId, had_state: bool },
}

impl<M, I, N, C> TraceEvent<M, I, N, C> {
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
            | TraceEvent::Resumed { at, .. }
            | TraceEvent::Restarted { at, .. }
            | TraceEvent::Wrote { at, .. }
            | TraceEvent::DiedWriting { at, .. }
            | TraceEvent::Said { at, .. }
            | TraceEvent::Invoked { at, .. }
            | TraceEvent::NotInvoked { at, .. }
            | TraceEvent::Recovered { at, .. } => *at,
        }
    }
}

/// An ordered log of everything a run did.
#[derive(Debug, Clone)]
pub struct Trace<M, I, N, C> {
    events: Vec<TraceEvent<M, I, N, C>>,
}

impl<M, I, N, C> Default for Trace<M, I, N, C> {
    fn default() -> Self {
        Trace { events: Vec::new() }
    }
}

impl<M, I, N, C> Trace<M, I, N, C> {
    pub(crate) fn push(&mut self, e: TraceEvent<M, I, N, C>) {
        self.events.push(e);
    }

    pub fn events(&self) -> &[TraceEvent<M, I, N, C>] {
        &self.events
    }

    /// Every operation that was handled, in order: which process, its identity, and the command.
    ///
    /// The left-hand ends of the intervals a checker needs. Pairing them with the indications that
    /// completed them is not something the trace does — see the `simulation` capability.
    pub fn invocations(&self) -> impl Iterator<Item = (NodeId, OpId, &C)> {
        self.events.iter().filter_map(|e| match e {
            TraceEvent::Invoked { node, op, cmd, .. } => Some((*node, *op, cmd)),
            _ => None,
        })
    }

    /// When `op` was handled, if it was.
    pub fn invoked_at(&self, op: OpId) -> Option<Time> {
        self.events.iter().find_map(|e| match e {
            TraceEvent::Invoked { at, op: o, .. } if *o == op => Some(*at),
            _ => None,
        })
    }

    /// Every operation that never reached the process it was given to, and why.
    pub fn not_begun(&self) -> impl Iterator<Item = (NodeId, OpId, NotBegun)> {
        self.events.iter().filter_map(|e| match e {
            TraceEvent::NotInvoked { node, op, why, .. } => Some((*node, *op, *why)),
            _ => None,
        })
    }

    /// Why `op` never began, if it did not. `None` covers both "it began" and "no such operation",
    /// which [`Trace::invocations`] distinguishes.
    pub fn why_not_begun(&self, op: OpId) -> Option<NotBegun> {
        self.not_begun().find(|(_, o, _)| *o == op).map(|(_, _, why)| why)
    }

    /// Every decision narrated, in order, with the process that narrated it.
    ///
    /// Empty unless the run was asked to record them — see `Sim::record_notes`.
    pub fn notes(&self) -> impl Iterator<Item = (NodeId, &N)> {
        self.events.iter().filter_map(|e| match e {
            TraceEvent::Said { node, note, .. } => Some((*node, note)),
            _ => None,
        })
    }

    /// Every decision narrated by `node`, in order.
    pub fn notes_at(&self, node: NodeId) -> impl Iterator<Item = &N> {
        self.notes().filter(move |(n, _)| *n == node).map(|(_, x)| x)
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

/// The trace type for a given protocol.
///
/// A trace names all four of a protocol's outward vocabularies — what it was asked, what crossed
/// the wire, what it concluded, what it said — so writing them out is four associated types every
/// time. The same shape as `recon_core::ProtoCx`, and for the same reason.
pub type ProtoTrace<P> =
    Trace<<P as Protocol>::Msg, <P as Protocol>::Ind, <P as Protocol>::Note, <P as Protocol>::Cmd>;

/// One event of a given protocol's trace.
pub type ProtoTraceEvent<P> = TraceEvent<
    <P as Protocol>::Msg,
    <P as Protocol>::Ind,
    <P as Protocol>::Note,
    <P as Protocol>::Cmd,
>;

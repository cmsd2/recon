//! What a protocol says about a decision it took.
//!
//! The simulator's trace records what happened *to* a protocol — what it sent, what was delivered,
//! what it wrote. It cannot record what the protocol **decided**, and it is completely silent about
//! a decision whose outcome was to do nothing. That is the case that has cost this project the most:
//! a leader trusted by everyone that announced nothing left no trace event at all, and finding it
//! meant bisecting the whole stack by hand.
//!
//! # A note earns its place only where the trace cannot say it
//!
//! The rule, and it is the one that keeps narration from decaying. A note beside
//! `cx.indicate(Ind::StartEpoch { ts, leader })` restating the same thing adds nothing a reader
//! could not already see, and the two can now disagree — which is exactly how the docstring that
//! quoted a deadlocking resend clause its own comment said a test had replaced went wrong.
//!
//! Three things qualify:
//!
//! - a decision that produced no effect at all — a message refused, a candidate already passed;
//! - *why* an effect was produced, where the effect alone is ambiguous — `epoch_change` sends the
//!   same `NACK` on the wire whether it is refusing an announcement or volunteering how far it has
//!   reached, and a reader of the trace cannot tell those apart;
//! - nothing else, yet. The vocabulary grows one narrated module at a time.
//!
//! # One vocabulary per run
//!
//! A note's type belongs to the run rather than to a layer, exactly as a `TimerId` does: a composed
//! stack narrates in one vocabulary, and a note passes through composition untouched, with no
//! mapper and nothing for a parent to re-wrap. A parent letting a child's note through is not
//! endorsing it — the decision was the child's.
//!
//! That is why every protocol in this crate declares `type Note = Note`, including the twenty-five
//! that narrate nothing. The alternative — each layer naming its own vocabulary and parents
//! converting — was built and discarded: it put a `where` clause relating two type parameters on
//! every layer generic over a port, to express something no layer actually wanted to do.

use recon_core::NodeId;

/// A decision a protocol took, in the vocabulary this stack narrates in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Note {
    /// An announcement of a new epoch was refused. The trace holds the refusal that went back on
    /// the wire; it does not hold what was wrong with the announcement.
    EpochRefused { from: NodeId, ts: u64, why: Refusal },
    /// A peer's report of how far it had reached was not acted on. **Nothing at all reaches the
    /// trace from this decision** — it is the shape of silence that cost the most to diagnose.
    ReportIgnored { from: NodeId, nts: u64, why: Refusal },
    /// This process volunteered how far it had reached to a leader that may never have been told it
    /// had become one. The same `NACK` as a refusal goes on the wire, so the trace cannot tell the
    /// two decisions apart; this says which it was.
    ReachReported { leader: NodeId, nts: u64 },
}

/// Why something was refused or ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The sender is not the process this one currently trusts.
    NotTrusted { trusted: NodeId },
    /// The timestamp offered is not above what this process has already reached.
    NotAhead { reached: u64 },
    /// This process does not trust itself, so acting on the report is not its business.
    NotLeader { trusted: NodeId },
}

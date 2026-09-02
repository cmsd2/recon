//! Rendering a run to a `tracing` subscriber as it happens.
//!
//! The trace is the record; this is a view of it. Both come from one producer in one order, so
//! there is no second account of the run to disagree with the first — which is the whole reason
//! narration goes into the trace rather than straight to a subscriber.
//!
//! # Why the driver renders, and not the protocol
//!
//! `tracing`'s dispatcher is thread-local. A protocol calling `tracing::info!` would be reaching
//! for something ambient, which constraint 2 exists to forbid, and it would get both of the facts a
//! reader needs first wrong: five simulated processes share one thread, so nothing would say *which*
//! process spoke, and a subscriber timestamps with the wall clock, which measures how long the
//! simulation took rather than anything about the run being reproduced.
//!
//! So protocols call `Cx::note`, the simulator records it, and only the simulator — a driver, which
//! is allowed to — touches a dispatcher.
//!
//! # As it is recorded, not at the end
//!
//! A run that fails to terminate is one of the things worth reading, and a renderer that walked a
//! finished trace would have nothing to show for it.

use crate::trace::{ProtoTraceEvent, TraceEvent};
use core::fmt::Debug;
use recon_core::Protocol;

/// Renders one recorded event. A function pointer rather than a closure so that the simulator can
/// hold it without acquiring `Debug` bounds it does not otherwise need — the same shape as the
/// codec check.
pub(crate) type Render<P> = fn(&ProtoTraceEvent<P>);

/// Emit one recorded event to whatever subscriber is installed.
///
/// `at` is the run's virtual time on every event, and `node` names the process wherever the event
/// has one. Everything the simulator does to a run is `DEBUG`; what a protocol *says* is `INFO`,
/// because that is the half a reader is usually after and the half that is rare.
pub(crate) fn render<P>(event: &ProtoTraceEvent<P>)
where
    P: Protocol,
    P::Msg: Debug,
    P::Ind: Debug,
    P::Note: Debug,
    P::Cmd: Debug,
{
    let at = event.at().as_offset();
    match event {
        TraceEvent::Said { node, note, .. } => {
            tracing::info!(target: "recon_sim", ?at, node = %node, ?note, "said");
        }
        TraceEvent::Invoked { node, op, cmd, .. } => {
            tracing::info!(target: "recon_sim", ?at, node = %node, %op, ?cmd, "invoked");
        }
        TraceEvent::NotInvoked { node, op, cmd, why, .. } => {
            tracing::info!(target: "recon_sim", ?at, node = %node, %op, ?cmd, ?why, "not invoked");
        }
        TraceEvent::Sent { from, to, msg, .. } => {
            tracing::debug!(target: "recon_sim", ?at, node = %from, %to, ?msg, "sent");
        }
        TraceEvent::Delivered { from, to, msg, .. } => {
            tracing::debug!(target: "recon_sim", ?at, node = %to, %from, ?msg, "delivered");
        }
        TraceEvent::Dropped { from, to, msg, reason, .. } => {
            tracing::debug!(target: "recon_sim", ?at, node = %from, %to, ?msg, ?reason, "dropped");
        }
        TraceEvent::Duplicated { from, to, msg, .. } => {
            tracing::debug!(target: "recon_sim", ?at, node = %from, %to, ?msg, "duplicated");
        }
        TraceEvent::Reordered { from, to, msg, .. } => {
            tracing::debug!(target: "recon_sim", ?at, node = %from, %to, ?msg, "reordered");
        }
        TraceEvent::TimerFired { node, id, .. } => {
            tracing::debug!(target: "recon_sim", ?at, node = %node, ?id, "timer fired");
        }
        TraceEvent::Indicated { node, ind, .. } => {
            tracing::debug!(target: "recon_sim", ?at, node = %node, ?ind, "indicated");
        }
        TraceEvent::SessionOpened { a, b, epoch, .. } => {
            tracing::debug!(target: "recon_sim", ?at, %a, %b, epoch, "session opened");
        }
        TraceEvent::SessionEnded { a, b, epoch, reason, .. } => {
            tracing::debug!(target: "recon_sim", ?at, %a, %b, epoch, ?reason, "session ended");
        }
        TraceEvent::SuffixLost { from, to, msg, .. } => {
            tracing::debug!(target: "recon_sim", ?at, node = %from, %to, ?msg, "suffix lost");
        }
        TraceEvent::Crashed { node, .. } => {
            tracing::debug!(target: "recon_sim", ?at, node = %node, "crashed");
        }
        TraceEvent::Suspended { node, .. } => {
            tracing::debug!(target: "recon_sim", ?at, node = %node, "suspended");
        }
        TraceEvent::Resumed { node, .. } => {
            tracing::debug!(target: "recon_sim", ?at, node = %node, "resumed");
        }
        TraceEvent::Restarted { node, .. } => {
            tracing::debug!(target: "recon_sim", ?at, node = %node, "restarted");
        }
        TraceEvent::Wrote { node, kind, .. } => {
            tracing::debug!(target: "recon_sim", ?at, node = %node, ?kind, "wrote");
        }
        TraceEvent::DiedWriting { node, .. } => {
            tracing::debug!(target: "recon_sim", ?at, node = %node, "died writing");
        }
        TraceEvent::Recovered { node, had_state, .. } => {
            tracing::debug!(target: "recon_sim", ?at, node = %node, had_state, "recovered");
        }
    }
}

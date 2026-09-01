//! Verifies that a protocol can narrate a decision, that the record lands in the same account as
//! what happened, and that narrating changes nothing.
//!
//! The protocol is deliberately trivial, so anything observed is the narration machinery's
//! behaviour and not an algorithm's. What a real module's notes are worth is asserted where the
//! module is — see `recon-protocols/tests/epoch_change.rs`.

use core::time::Duration;
use recon_core::{NodeId, ProtoCx, Protocol, TimerId};
use recon_sim::{Config, Sim, TraceEvent};
use std::sync::{Arc, Mutex};
use tracing_subscriber::layer::SubscriberExt;

const A: NodeId = NodeId::new(1);
const B: NodeId = NodeId::new(2);

/// Decides whether a number is worth forwarding, and says so either way. The interesting half is
/// the refusal: nothing about it would otherwise reach the trace.
struct Picky;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Cmd {
    Offer(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Wire(u32);

#[derive(Debug, Clone, PartialEq, Eq)]
struct Got(u32);

#[derive(Debug, Clone, PartialEq, Eq)]
enum Note {
    /// Decided to forward. An effect follows, so this says only *why*.
    Forwarding { n: u32 },
    /// Decided not to. **Nothing at all reaches the trace from this decision.**
    Refused { n: u32, because: &'static str },
}

impl Protocol for Picky {
    type Cmd = Cmd;
    type Ind = Got;
    type Msg = Wire;
    type Scope = core::convert::Infallible;
    type Note = Note;
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    fn on_cmd(&mut self, Cmd::Offer(n): Cmd, cx: &mut ProtoCx<'_, Self>) {
        if n % 2 == 0 {
            cx.note(Note::Forwarding { n });
            cx.send(B, Wire(n));
        } else {
            cx.note(Note::Refused { n, because: "odd" });
        }
    }

    fn on_msg(&mut self, _: NodeId, Wire(n): Wire, cx: &mut ProtoCx<'_, Self>) {
        cx.indicate(Got(n));
    }

    fn on_timer(&mut self, _: TimerId, _: &mut ProtoCx<'_, Self>) {}
}

fn sim(record: bool) -> Sim<Picky> {
    let mut s = Sim::new(Config::default().seed(3), &[A, B], |_| Picky);
    if record {
        s.record_notes();
    }
    s
}

fn drive(s: &mut Sim<Picky>) {
    s.command(A, Cmd::Offer(2));
    s.command(A, Cmd::Offer(7));
    s.run_for(Duration::from_millis(50));
}

// ------------------------------------------------------------------ in the trace

#[test]
fn a_narrated_decision_reaches_the_trace_with_the_process_and_the_instant() {
    let mut s = sim(true);
    s.run_for(Duration::from_millis(5));
    let at = s.now();
    s.command(A, Cmd::Offer(7));
    s.step_now();

    let said: Vec<&TraceEvent<Wire, Got, Note>> =
        s.trace().events().iter().filter(|e| matches!(e, TraceEvent::Said { .. })).collect();
    assert_eq!(said.len(), 1);
    let TraceEvent::Said { at: said_at, node, note } = said[0] else { unreachable!() };
    assert_eq!(*node, A);
    assert_eq!(*said_at, at);
    assert_eq!(*note, Note::Refused { n: 7, because: "odd" });
}

/// The decision, then what the decision led to.
#[test]
fn a_note_precedes_the_effects_of_its_handler() {
    let mut s = sim(true);
    drive(&mut s);

    let events = s.trace().events();
    let note = events
        .iter()
        .position(|e| matches!(e, TraceEvent::Said { note: Note::Forwarding { n: 2 }, .. }))
        .expect("the forwarding decision was narrated");
    let sent = events
        .iter()
        .position(|e| matches!(e, TraceEvent::Sent { msg: Wire(2), .. }))
        .expect("and the message went");
    assert!(note < sent, "the decision must come before what it led to");
}

/// The one a refusal is for: it leaves no other mark.
#[test]
fn a_decision_to_do_nothing_leaves_only_its_note() {
    let mut s = sim(true);
    s.command(A, Cmd::Offer(7));
    s.run_for(Duration::from_millis(50));

    assert_eq!(s.trace().notes_at(A).count(), 1);
    assert_eq!(s.trace().send_count(), 0, "nothing else records the refusal");
    assert_eq!(s.trace().indication_count(), 0);
}

// ------------------------------------------------------------------ it changes nothing

/// Without this, narration would be a fault injector: the runs that are read would not be the runs
/// that fail, and every diagnosis reached by reading one would be a diagnosis of a different run.
#[test]
fn narrating_does_not_change_the_run() {
    let strip = |s: &Sim<Picky>| -> Vec<TraceEvent<Wire, Got, Note>> {
        s.trace()
            .events()
            .iter()
            .filter(|e| !matches!(e, TraceEvent::Said { .. }))
            .cloned()
            .collect()
    };

    let mut heard = sim(true);
    let mut unheard = sim(false);
    drive(&mut heard);
    drive(&mut unheard);

    assert_eq!(strip(&heard), strip(&unheard));
    // Non-vacuity: the run being compared must actually have done something, and the observed one
    // must actually have narrated.
    assert!(strip(&heard).len() >= 3, "a run with nothing in it would agree trivially");
    assert!(heard.trace().notes().count() > 0);
}

#[test]
fn a_run_without_an_audience_records_nothing_said() {
    let mut s = sim(false);
    drive(&mut s);
    assert_eq!(s.trace().notes().count(), 0);
    assert!(s.trace().send_count() > 0, "but the run still happened");
}

// ------------------------------------------------------------------ rendering

/// Captures whatever a `tracing` subscriber would have shown, as field text.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<String>>>);

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Captured {
    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        struct Fields(String);
        impl tracing::field::Visit for Fields {
            fn record_debug(&mut self, f: &tracing::field::Field, v: &dyn core::fmt::Debug) {
                self.0.push_str(&format!("{}={v:?} ", f.name()));
            }
        }
        let mut fields = Fields(String::new());
        event.record(&mut fields);
        self.0.lock().expect("not poisoned").push(fields.0);
    }
}

fn captured() -> (Captured, tracing::subscriber::DefaultGuard) {
    // Per-thread rather than global: tests run in parallel, and a global subscriber can be
    // installed only once per process.
    let sink = Captured::default();
    let guard = tracing::subscriber::set_default(
        tracing_subscriber::registry()
            .with(sink.clone())
            .with(tracing::level_filters::LevelFilter::TRACE),
    );
    (sink, guard)
}

/// A run that never terminates is one of the things worth reading, so events must already have
/// been emitted while it is still going.
#[test]
fn a_run_still_going_has_already_reported() {
    let (sink, _guard) = captured();
    let mut s = sim(true);
    s.enable_tracing();
    s.command(A, Cmd::Offer(2));
    s.command(A, Cmd::Offer(7));

    s.step_now();
    let so_far = sink.0.lock().expect("not poisoned").len();
    assert!(so_far > 0, "nothing was emitted before the run finished");
    assert!(!s.trace().events().is_empty());

    s.run_for(Duration::from_millis(50));
    assert!(sink.0.lock().expect("not poisoned").len() > so_far, "and it kept reporting");
}

#[test]
fn what_is_rendered_carries_the_process_and_the_runs_own_time() {
    let (sink, _guard) = captured();
    let mut s = sim(true);
    s.enable_tracing();
    s.run_for(Duration::from_millis(7));
    s.command(A, Cmd::Offer(7));
    s.step_now();

    let lines = sink.0.lock().expect("not poisoned").clone();
    let said = lines.iter().find(|l| l.contains("Refused")).expect("the refusal was rendered");
    assert!(said.contains("node=n1"), "{said}");
    // The run's own clock: seven milliseconds in, whatever the wall clock says.
    assert!(said.contains("at=7ms"), "{said}");
}

#[test]
fn a_run_without_tracing_renders_nothing() {
    let (sink, _guard) = captured();
    let mut s = sim(true);
    drive(&mut s);
    assert!(sink.0.lock().expect("not poisoned").is_empty());
    assert!(!s.trace().events().is_empty(), "but the run still happened");
}

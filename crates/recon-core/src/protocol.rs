//! The protocol contract.

use crate::{Cx, Effect, NodeId, Time};
use rand::RngCore;

/// A synchronous protocol state machine.
///
/// Implementors handle three kinds of event — a command from the layer above, a message from a
/// peer, and a timer fire — and emit effects through the context. Handling an event runs to
/// completion: it cannot await, cannot be suspended, and cannot be cancelled part-way, so a
/// state transition is atomic with respect to whatever drives it.
///
/// The four associated types are the protocol's ports, in Kompics terms: what it accepts from
/// above, what it promises upward, what it puts on the wire, and what it schedules.
pub trait Protocol {
    /// Requests from the layer above.
    type Cmd;
    /// Indications to the layer above — this protocol delivering on its guarantee.
    type Ind;
    /// What crosses the wire to a peer running the same protocol.
    type Msg;
    /// Tokens identifying this protocol's own timers.
    type Timer;

    /// What this protocol writes down so that it survives a crash.
    ///
    /// The value carried by [`Effect::Store`] is this type *in full*, not a delta: recovery hands
    /// back the last one written and nothing else. Declaring it is how a reader learns what a
    /// process would still know after restarting, without having to work out which fields happen
    /// to get written.
    ///
    /// A protocol that keeps nothing durably declares [`core::convert::Infallible`], and then no
    /// store effect can be constructed for it and [`Protocol::on_recovery`] can never be called —
    /// the same check-rather-than-trust that [`Protocol::Scope`] uses.
    ///
    /// A parent may compose only a child whose durable type is uninhabited. There is no mapping
    /// from a child's durable state into a parent's, because a parent's contains its own fields
    /// as well; see [`crate::effect::absurd`].
    type Durable;

    /// Scopes whose ending this protocol's guarantees depend on, and whose ends it can observe.
    ///
    /// A guarantee is rarely absolute. It holds while some condition does — a transport session,
    /// a retention window — and the end of that condition is an event the protocol must be told
    /// about, not an implementation detail beneath it. See `docs/scope-annotated-modules.md`.
    ///
    /// A protocol with no such condition declares [`core::convert::Infallible`]. That is not a
    /// convention: an uninhabited type has no values, so a scope ending cannot be constructed for
    /// it and [`Protocol::on_scope_end`] can never be called. The absence is checked rather than
    /// trusted, and such a protocol writes no handler at all.
    ///
    /// A scope may only be named by a protocol that can observe its end. Naming one it cannot
    /// detect creates an obligation no implementation can discharge and no test can exercise.
    type Scope;

    /// Handle a request from the layer above.
    fn on_cmd(&mut self, cmd: Self::Cmd, cx: &mut ProtoCx<'_, Self>);

    /// Handle a message received from `from`.
    fn on_msg(&mut self, from: NodeId, msg: Self::Msg, cx: &mut ProtoCx<'_, Self>);

    /// Handle a timer this protocol previously set.
    fn on_timer(&mut self, token: Self::Timer, cx: &mut ProtoCx<'_, Self>);

    /// Begin, on a first start — when nothing has been written down.
    ///
    /// Exactly one of this and [`Protocol::on_recovery`] runs at startup, which is the branch the
    /// book draws with `⟨ Init ⟩` and `⟨ Recovery ⟩`. It matters because some first-start work must
    /// *not* happen on a restart: writing an initial value down is the standard case, and doing it
    /// again on recovery would overwrite what was being recovered.
    ///
    /// The constructor cannot serve this purpose. It runs in both cases — it is the common prefix
    /// of the two branches, not one of them — and it cannot emit effects, so it has nowhere to put
    /// a write. Volatile setup belongs there; anything a first start must *do* belongs here.
    fn on_init(&mut self, _cx: &mut ProtoCx<'_, Self>) {}

    /// Resume from durable state after a crash.
    ///
    /// Distinct from construction, and deliberately: the algorithms that need it *act* on
    /// recovering — re-announcing the log they had already delivered, re-sending what was still
    /// pending — and those are effects, which a constructor cannot emit.
    ///
    /// Volatile state is whatever `new` established; `durable` is the last value stored before
    /// the crash. Exactly one of this and [`Protocol::on_init`] runs at startup: a process with
    /// nothing in storage is initialised, one with something is recovered, never both.
    ///
    /// The default does nothing, which is unreachable for a protocol whose `Durable` is
    /// uninhabited.
    fn on_recovery(&mut self, _durable: Self::Durable, _cx: &mut ProtoCx<'_, Self>) {}

    /// Handle the ending of a scope this protocol's guarantees depended on.
    ///
    /// Scope endings travel *downward*, like messages: they originate outside the stack and are
    /// routed by each layer to whichever child cares. What travels back up is an indication —
    /// a layer that cannot restore its guarantee says so in its own terms.
    ///
    /// The default does nothing, which is unreachable for a protocol whose `Scope` is uninhabited.
    fn on_scope_end(&mut self, _scope: Self::Scope, _cx: &mut ProtoCx<'_, Self>) {}
}

/// The context type for a given protocol.
pub type ProtoCx<'a, P> = Cx<
    'a,
    <P as Protocol>::Msg,
    <P as Protocol>::Ind,
    <P as Protocol>::Timer,
    <P as Protocol>::Durable,
>;

/// The effect type for a given protocol.
pub type ProtoEffect<P> = Effect<
    <P as Protocol>::Msg,
    <P as Protocol>::Ind,
    <P as Protocol>::Timer,
    <P as Protocol>::Durable,
>;

/// An event a protocol can be given. Used by drivers and by the test helper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event<C, M, T, S, D> {
    Cmd(C),
    Msg {
        from: NodeId,
        msg: M,
    },
    Timer(T),
    /// A scope this protocol's guarantees depended on has ended.
    ScopeEnd(S),
    /// This process is starting for the first time, with nothing written down.
    Init,
    /// This process restarted, and here is what it had written down.
    Recovery(D),
}

/// The event type for a given protocol.
pub type ProtoEvent<P> = Event<
    <P as Protocol>::Cmd,
    <P as Protocol>::Msg,
    <P as Protocol>::Timer,
    <P as Protocol>::Scope,
    <P as Protocol>::Durable,
>;

/// Deliver one event to `p` and return the effects it emitted.
///
/// Restores the ergonomics of a pure function for tests — `assert_eq!(step(..), [..])` — without
/// making production paths allocate a vector per event. Intended for tests; drivers own a
/// reusable buffer and call the handlers directly.
pub fn step<P: Protocol + ?Sized>(
    p: &mut P,
    event: ProtoEvent<P>,
    now: Time,
    rng: &mut dyn RngCore,
) -> Vec<ProtoEffect<P>> {
    let mut effects = Vec::new();
    {
        let mut cx = Cx::new(&mut effects, now, rng);
        match event {
            Event::Cmd(c) => p.on_cmd(c, &mut cx),
            Event::Msg { from, msg } => p.on_msg(from, msg, &mut cx),
            Event::Timer(t) => p.on_timer(t, &mut cx),
            Event::ScopeEnd(s) => p.on_scope_end(s, &mut cx),
            Event::Init => p.on_init(&mut cx),
            Event::Recovery(d) => p.on_recovery(d, &mut cx),
        }
    }
    effects
}

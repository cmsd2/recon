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

    /// The durable value this protocol rewrites: a position, a count, an epoch. Small enough that
    /// rewriting it costs nothing.
    ///
    /// A protocol keeping nothing durably declares this and [`Protocol::Entry`] as
    /// [`core::convert::Infallible`], and then a write cannot be constructed — the same
    /// check-rather-than-trust that [`Protocol::Scope`] uses.
    type Meta;

    /// The durable entries this protocol appends: what accumulates.
    type Entry;

    /// Scopes whose boundaries this protocol's guarantees depend on, and which it can observe.
    ///
    /// A guarantee is rarely absolute. It holds while some condition does — a transport session,
    /// a retention window — and the end of that condition is an event the protocol must be told
    /// about, not an implementation detail beneath it. See `docs/scope-annotated-modules.md`.
    ///
    /// **Both** boundaries travel here, not only the ending. A scope beginning is what makes a
    /// bridge possible at all: an ending says a suffix may be gone, and only the beginning of the
    /// successor says where to send it again. A port carrying just the ending would name a
    /// problem with no event on which to act.
    ///
    /// A protocol with no such condition declares [`core::convert::Infallible`]. That is not a
    /// convention: an uninhabited type has no values, so a scope event cannot be constructed for
    /// it and [`Protocol::on_scope_event`] can never be called. The absence is checked rather than
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

    /// Resume after a crash, reading what survived.
    ///
    /// Distinct from construction, and deliberately: the algorithms that need it *act* on
    /// recovering — re-announcing what they had already delivered, re-sending what was still
    /// pending — and those are effects, which a constructor cannot emit.
    ///
    /// What survived is read from [`Cx::storage`] rather than handed over, since a protocol may
    /// need only part of it. Exactly one of this and [`Protocol::on_init`] runs at startup.
    ///
    /// Nothing else is dispatched between being told to recover and this returning, which is what
    /// makes it safe to hold state not yet loaded.
    fn on_recovery(&mut self, _cx: &mut ProtoCx<'_, Self>) {}

    /// Handle a boundary of a scope this protocol's guarantees depend on — its end, or the
    /// beginning of the one that succeeds it.
    ///
    /// Scope events travel *downward*, like messages: they originate outside the stack and are
    /// routed by each layer to whichever child cares, in the concrete type the bottom layer
    /// declares. What travels back up is an indication — a layer that cannot restore its
    /// guarantee says so in its own terms.
    ///
    /// The default does nothing, which is unreachable for a protocol whose `Scope` is uninhabited.
    fn on_scope_event(&mut self, _scope: Self::Scope, _cx: &mut ProtoCx<'_, Self>) {}
}

/// The context type for a given protocol.
pub type ProtoCx<'a, P> = Cx<
    'a,
    <P as Protocol>::Msg,
    <P as Protocol>::Ind,
    <P as Protocol>::Timer,
    <P as Protocol>::Meta,
    <P as Protocol>::Entry,
>;

/// The effect type for a given protocol.
pub type ProtoEffect<P> =
    Effect<<P as Protocol>::Msg, <P as Protocol>::Ind, <P as Protocol>::Timer>;

/// An event a protocol can be given. Used by drivers and by the test helper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event<C, M, T, S> {
    Cmd(C),
    Msg {
        from: NodeId,
        msg: M,
    },
    Timer(T),
    /// A scope this protocol's guarantees depended on has ended.
    ScopeEvent(S),
    /// This process is starting for the first time, with nothing written down.
    Init,
    /// This process restarted, and something it wrote down survived.
    Recovery,
}

/// The event type for a given protocol.
pub type ProtoEvent<P> = Event<
    <P as Protocol>::Cmd,
    <P as Protocol>::Msg,
    <P as Protocol>::Timer,
    <P as Protocol>::Scope,
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
    let mut store = crate::store::MemStore::default();
    step_in(p, event, now, rng, &mut store)
}

/// Deliver one event to `p` against a store the caller owns, and return the effects it emitted.
///
/// [`step`] gives the protocol a fresh store each call, which is right for one that keeps nothing
/// durably and wrong for one that does — a write in one call would be invisible in the next. A
/// test that cares about what survives passes its own store here and can inspect it afterwards.
pub fn step_in<P: Protocol + ?Sized>(
    p: &mut P,
    event: ProtoEvent<P>,
    now: Time,
    rng: &mut dyn RngCore,
    store: &mut dyn crate::store::Store<P::Meta, P::Entry>,
) -> Vec<ProtoEffect<P>> {
    let mut effects = Vec::new();
    {
        let mut cx = Cx::new(&mut effects, now, rng, store);
        match event {
            Event::Cmd(c) => p.on_cmd(c, &mut cx),
            Event::Msg { from, msg } => p.on_msg(from, msg, &mut cx),
            Event::Timer(t) => p.on_timer(t, &mut cx),
            Event::ScopeEvent(s) => p.on_scope_event(s, &mut cx),
            Event::Init => p.on_init(&mut cx),
            Event::Recovery => p.on_recovery(&mut cx),
        }
    }
    effects
}

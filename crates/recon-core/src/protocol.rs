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

    /// Handle a request from the layer above.
    fn on_cmd(&mut self, cmd: Self::Cmd, cx: &mut ProtoCx<'_, Self>);

    /// Handle a message received from `from`.
    fn on_msg(&mut self, from: NodeId, msg: Self::Msg, cx: &mut ProtoCx<'_, Self>);

    /// Handle a timer this protocol previously set.
    fn on_timer(&mut self, token: Self::Timer, cx: &mut ProtoCx<'_, Self>);
}

/// The context type for a given protocol.
pub type ProtoCx<'a, P> =
    Cx<'a, <P as Protocol>::Msg, <P as Protocol>::Ind, <P as Protocol>::Timer>;

/// The effect type for a given protocol.
pub type ProtoEffect<P> =
    Effect<<P as Protocol>::Msg, <P as Protocol>::Ind, <P as Protocol>::Timer>;

/// An event a protocol can be given. Used by drivers and by the test helper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event<C, M, T> {
    Cmd(C),
    Msg { from: NodeId, msg: M },
    Timer(T),
}

/// The event type for a given protocol.
pub type ProtoEvent<P> = Event<<P as Protocol>::Cmd, <P as Protocol>::Msg, <P as Protocol>::Timer>;

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
        }
    }
    effects
}

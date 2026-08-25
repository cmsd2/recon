//! Where a protocol's effects go, and how it is told the time.

use crate::{Effect, NodeId, Time};
use core::time::Duration;
use rand::RngCore;

/// Receives the effects a protocol emits.
///
/// The core deliberately has no opinion about how effects are stored. A driver that wants
/// amortised allocation passes a `Vec` and reuses it across events; a `no_std` or
/// latency-sensitive driver passes a fixed-capacity sink; a test passes one that merely counts.
/// Protocol code is identical in every case, because a protocol only ever calls
/// [`Cx::send`] and friends.
pub trait EffectSink<M, I, T> {
    fn emit(&mut self, effect: Effect<M, I, T>);
}

impl<M, I, T> EffectSink<M, I, T> for Vec<Effect<M, I, T>> {
    fn emit(&mut self, effect: Effect<M, I, T>) {
        self.push(effect);
    }
}

/// Translates a child's effects into a parent's terms as they are emitted.
///
/// This is what makes composition free of intermediate buffers: the child pushes, the mapper
/// re-wraps, and the parent's sink receives — in one step, with nothing collected in between.
struct MapSink<'p, PM, PI, PT, CM, CI, CT> {
    parent: &'p mut dyn EffectSink<PM, PI, PT>,
    msg: fn(CM) -> PM,
    ind: fn(CI) -> PI,
    timer: fn(CT) -> PT,
}

impl<PM, PI, PT, CM, CI, CT> EffectSink<CM, CI, CT> for MapSink<'_, PM, PI, PT, CM, CI, CT> {
    fn emit(&mut self, effect: Effect<CM, CI, CT>) {
        self.parent.emit(effect.map(self.msg, self.ind, self.timer));
    }
}

/// A protocol's window onto the world.
///
/// Supplies the current time and a seeded randomness source, and receives every effect the
/// protocol emits. Nothing else reaches a protocol: given the same state, event, `now`, and RNG
/// stream, it behaves identically every time.
pub struct Cx<'a, M, I, T> {
    sink: &'a mut dyn EffectSink<M, I, T>,
    now: Time,
    rng: &'a mut dyn RngCore,
}

impl<'a, M, I, T> Cx<'a, M, I, T> {
    /// Build a context over any sink.
    pub fn new(sink: &'a mut dyn EffectSink<M, I, T>, now: Time, rng: &'a mut dyn RngCore) -> Self {
        Cx { sink, now, rng }
    }

    /// Transmit `msg` to `to`.
    pub fn send(&mut self, to: NodeId, msg: M) {
        self.sink.emit(Effect::Send { to, msg });
    }

    /// Raise an indication to the layer above.
    pub fn indicate(&mut self, ind: I) {
        self.sink.emit(Effect::Indicate(ind));
    }

    /// Ask for `token` to be handed back after `after`.
    pub fn set_timer(&mut self, after: Duration, token: T) {
        self.sink.emit(Effect::SetTimer { after, token });
    }

    /// The current time — virtual under simulation, real under a live driver.
    pub fn now(&self) -> Time {
        self.now
    }

    /// The seeded randomness source. Reproducible for a given seed.
    pub fn rng(&mut self) -> &mut dyn RngCore {
        self.rng
    }

    /// Run `f` against a child's context, translating everything it emits into this protocol's
    /// terms on the way out.
    ///
    /// The three mappers are normally enum variant constructors, so a wrong one is a type error.
    /// Nothing is buffered: a child's effect is re-wrapped as it is emitted and passed straight
    /// on to this context's sink.
    pub fn with_child<CM, CI, CT>(
        &mut self,
        msg: fn(CM) -> M,
        ind: fn(CI) -> I,
        timer: fn(CT) -> T,
        f: impl FnOnce(&mut Cx<'_, CM, CI, CT>),
    ) {
        let mut mapped = MapSink { parent: &mut *self.sink, msg, ind, timer };
        let mut child = Cx { sink: &mut mapped, now: self.now, rng: &mut *self.rng };
        f(&mut child);
    }
}

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
pub trait EffectSink<M, I, T, D> {
    fn emit(&mut self, effect: Effect<M, I, T, D>);
}

impl<M, I, T, D> EffectSink<M, I, T, D> for Vec<Effect<M, I, T, D>> {
    fn emit(&mut self, effect: Effect<M, I, T, D>) {
        self.push(effect);
    }
}

/// Translates a child's effects into a parent's terms as they are emitted.
///
/// This is what makes composition free of intermediate buffers: the child pushes, the mapper
/// re-wraps, and the parent's sink receives — in one step, with nothing collected in between.
struct MapSink<'p, PM, PI, PT, PD, CM, CI, CT, CD> {
    parent: &'p mut dyn EffectSink<PM, PI, PT, PD>,
    msg: fn(CM) -> PM,
    ind: fn(CI) -> PI,
    timer: fn(CT) -> PT,
    durable: fn(CD) -> PD,
}

impl<PM, PI, PT, PD, CM, CI, CT, CD> EffectSink<CM, CI, CT, CD>
    for MapSink<'_, PM, PI, PT, PD, CM, CI, CT, CD>
{
    fn emit(&mut self, effect: Effect<CM, CI, CT, CD>) {
        self.parent.emit(effect.map(self.msg, self.ind, self.timer, self.durable));
    }
}

/// Translates a child's outgoing effects, but hands its indications back to the parent.
///
/// A parent almost never wants to forward a child's indications untouched: the child reporting
/// "a message arrived" is an *input* to the parent's logic, not an output of it. The parent
/// cannot react during the call — it is already borrowed by the child — so indications are
/// collected and processed once the child returns.
struct ConsumeSink<'p, 'c, PM, PI, PT, PD, CM, CI, CT, CD> {
    parent: &'p mut dyn EffectSink<PM, PI, PT, PD>,
    collected: &'c mut Vec<CI>,
    msg: fn(CM) -> PM,
    timer: fn(CT) -> PT,
    durable: fn(CD) -> PD,
}

impl<PM, PI, PT, PD, CM, CI, CT, CD> EffectSink<CM, CI, CT, CD>
    for ConsumeSink<'_, '_, PM, PI, PT, PD, CM, CI, CT, CD>
{
    fn emit(&mut self, effect: Effect<CM, CI, CT, CD>) {
        match effect {
            Effect::Send { to, msg } => self.parent.emit(Effect::Send { to, msg: (self.msg)(msg) }),
            Effect::SetTimer { after, token } => {
                self.parent.emit(Effect::SetTimer { after, token: (self.timer)(token) })
            }
            Effect::Store(d) => self.parent.emit(Effect::Store((self.durable)(d))),
            Effect::Indicate(ind) => self.collected.push(ind),
        }
    }
}

/// A protocol's window onto the world.
///
/// Supplies the current time and a seeded randomness source, and receives every effect the
/// protocol emits. Nothing else reaches a protocol: given the same state, event, `now`, and RNG
/// stream, it behaves identically every time.
pub struct Cx<'a, M, I, T, D> {
    sink: &'a mut dyn EffectSink<M, I, T, D>,
    now: Time,
    rng: &'a mut dyn RngCore,
}

impl<'a, M, I, T, D> Cx<'a, M, I, T, D> {
    /// Build a context over any sink.
    pub fn new(
        sink: &'a mut dyn EffectSink<M, I, T, D>,
        now: Time,
        rng: &'a mut dyn RngCore,
    ) -> Self {
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

    /// Write this protocol's durable state, so that it survives a crash.
    ///
    /// The write is durable before anything sent afterwards leaves the process, so a protocol may
    /// record a promise and make it in response to the same event.
    pub fn store(&mut self, durable: D) {
        self.sink.emit(Effect::Store(durable));
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
    /// The mappers are normally enum variant constructors, so a wrong one is a type error. The
    /// durable mapper is [`crate::effect::absurd`] wherever the child keeps nothing durably, which
    /// is every composition in this repository; a child that does keep durable state has no mapper
    /// that can be written, so composing one fails to build.
    /// Nothing is buffered: a child's effect is re-wrapped as it is emitted and passed straight
    /// on to this context's sink.
    pub fn with_child<CM, CI, CT, CD>(
        &mut self,
        msg: fn(CM) -> M,
        ind: fn(CI) -> I,
        timer: fn(CT) -> T,
        durable: fn(CD) -> D,
        f: impl FnOnce(&mut Cx<'_, CM, CI, CT, CD>),
    ) {
        let mut mapped = MapSink { parent: &mut *self.sink, msg, ind, timer, durable };
        let mut child = Cx { sink: &mut mapped, now: self.now, rng: &mut *self.rng };
        f(&mut child);
    }

    /// Run `f` against a child's context, forwarding what it sends and schedules but collecting
    /// its indications into `collected` for this protocol to handle itself.
    ///
    /// This is the usual shape. A child's indication is the parent's input — the stubborn link
    /// reporting a delivery is what the perfect link deduplicates — so it must be consumed, not
    /// passed through. `collected` belongs to the caller and is reused across events.
    pub fn with_child_consuming<CM, CI, CT, CD>(
        &mut self,
        msg: fn(CM) -> M,
        timer: fn(CT) -> T,
        durable: fn(CD) -> D,
        collected: &mut Vec<CI>,
        f: impl FnOnce(&mut Cx<'_, CM, CI, CT, CD>),
    ) {
        let mut sink = ConsumeSink { parent: &mut *self.sink, collected, msg, timer, durable };
        let mut child = Cx { sink: &mut sink, now: self.now, rng: &mut *self.rng };
        f(&mut child);
    }
}

//! Where a protocol's effects go, and how it is told the time.

use crate::store::{NoStore, Slot, SlotStore, Store};
use crate::{Effect, NodeId, Time, TimerId};
use core::convert::Infallible;
use core::time::Duration;
use rand::RngCore;

/// Receives the effects a protocol emits.
///
/// The core deliberately has no opinion about how effects are stored. A driver that wants
/// amortised allocation passes a `Vec` and reuses it across events; a `no_std` or
/// latency-sensitive driver passes a fixed-capacity sink; a test passes one that merely counts.
/// Protocol code is identical in every case, because a protocol only ever calls
/// [`Cx::send`] and friends.
pub trait EffectSink<M, I> {
    fn emit(&mut self, effect: Effect<M, I>);
}

impl<M, I> EffectSink<M, I> for Vec<Effect<M, I>> {
    fn emit(&mut self, effect: Effect<M, I>) {
        self.push(effect);
    }
}

/// Receives the decisions a protocol narrates.
///
/// Separate from [`EffectSink`], and deliberately. An effect is *deferred*: it describes something
/// the driver will do on the protocol's behalf. A note describes something that has **already
/// happened**, at a point inside the handler — so it is recorded at the moment of the call, in the
/// handler's own text, for the same reason [`crate::Store`] is not an effect either.
///
/// Nothing a protocol can observe reveals whether anything is listening, so no behaviour can
/// depend on it: a run is identical whether or not it was read.
pub trait NoteSink<N> {
    fn note(&mut self, note: N);
}

impl<N> NoteSink<N> for Vec<N> {
    fn note(&mut self, note: N) {
        self.push(note);
    }
}

/// Nobody is listening.
///
/// The ordinary case: a run pays for narration only when something asked to read it. A driver
/// keeps one of these and hands it to every context it builds, exactly as it would a real sink —
/// so that a protocol's code is identical either way, which is what makes narrating unable to
/// change the run.
pub struct NoNotes;

impl<N> NoteSink<N> for NoNotes {
    fn note(&mut self, _note: N) {}
}

/// Translates a child's effects into a parent's terms as they are emitted.
///
/// This is what makes composition free of intermediate buffers: the child pushes, the mapper
/// re-wraps, and the parent's sink receives — in one step, with nothing collected in between.
struct MapSink<'p, PM, PI, CM, CI> {
    parent: &'p mut dyn EffectSink<PM, PI>,
    msg: fn(CM) -> PM,
    ind: fn(CI) -> PI,
}

impl<PM, PI, CM, CI> EffectSink<CM, CI> for MapSink<'_, PM, PI, CM, CI> {
    fn emit(&mut self, effect: Effect<CM, CI>) {
        self.parent.emit(effect.map(self.msg, self.ind));
    }
}

/// Translates a child's outgoing effects, but hands its indications back to the parent.
///
/// A parent almost never wants to forward a child's indications untouched: the child reporting
/// "a message arrived" is an *input* to the parent's logic, not an output of it. The parent
/// cannot react during the call — it is already borrowed by the child — so indications are
/// collected and processed once the child returns.
struct ConsumeSink<'p, 'c, PM, PI, CM, CI> {
    parent: &'p mut dyn EffectSink<PM, PI>,
    collected: &'c mut Vec<CI>,
    msg: fn(CM) -> PM,
}

impl<PM, PI, CM, CI> EffectSink<CM, CI> for ConsumeSink<'_, '_, PM, PI, CM, CI> {
    fn emit(&mut self, effect: Effect<CM, CI>) {
        match effect {
            Effect::Send { to, msg } => self.parent.emit(Effect::Send { to, msg: (self.msg)(msg) }),
            // A timer belongs to whoever registered it, so it passes straight through: there is
            // nothing in it for a parent to re-wrap.
            Effect::SetTimer { after, id } => self.parent.emit(Effect::SetTimer { after, id }),
            Effect::Indicate(ind) => self.collected.push(ind),
        }
    }
}

/// A protocol's window onto the world.
///
/// Supplies the current time and a seeded randomness source, and receives every effect the
/// protocol emits. Nothing else reaches a protocol: given the same state, event, `now`, and RNG
/// stream, it behaves identically every time.
pub struct Cx<'a, M, I, N, Me, En> {
    sink: &'a mut dyn EffectSink<M, I>,
    now: Time,
    rng: &'a mut dyn RngCore,
    store: &'a mut dyn Store<Me, En>,
    /// Where narrated decisions go. Shared down the whole composition like `next_timer`, so a
    /// child's note reaches the run without the parent handling it. [`NoNotes`] when nobody is
    /// listening, which is the ordinary case.
    notes: &'a mut dyn NoteSink<N>,
    /// Where registered timers get their identities. Owned by the driver and shared down the whole
    /// composition, so an identity is unique to a run rather than to a layer — two layers each
    /// starting from zero would each accept the other's expiry.
    next_timer: &'a mut u64,
}

impl<'a, M, I, N, Me, En> Cx<'a, M, I, N, Me, En> {
    /// Build a context over any sink, any store, and any audience for what it narrates.
    ///
    /// Pass [`NoNotes`] when nothing is listening, which is the ordinary case. It is a parameter
    /// rather than an option so that the protocol's own code is the same either way — which is
    /// what makes narrating unable to change the run.
    pub fn new(
        sink: &'a mut dyn EffectSink<M, I>,
        now: Time,
        rng: &'a mut dyn RngCore,
        store: &'a mut dyn Store<Me, En>,
        next_timer: &'a mut u64,
        notes: &'a mut dyn NoteSink<N>,
    ) -> Self {
        Cx { sink, now, rng, store, next_timer, notes }
    }

    /// Transmit `msg` to `to`.
    pub fn send(&mut self, to: NodeId, msg: M) {
        self.sink.emit(Effect::Send { to, msg });
    }

    /// Raise an indication to the layer above.
    pub fn indicate(&mut self, ind: I) {
        self.sink.emit(Effect::Indicate(ind));
    }

    /// Register a timer for `after`, and take a handle naming it.
    ///
    /// The handle is what a later expiry is compared against, so a protocol can tell the timer it
    /// is waiting on from one it has superseded. It says nothing about which protocol registered
    /// it or where that protocol sits in a composition.
    pub fn set_timer(&mut self, after: Duration) -> TimerId {
        let id = TimerId(*self.next_timer);
        *self.next_timer += 1;
        self.sink.emit(Effect::SetTimer { after, id });
        id
    }

    /// Record a decision this protocol has taken.
    ///
    /// Recorded at the point of the call, not deferred like an effect: a note says what has already
    /// happened, and its place in the run is where the handler put it.
    ///
    /// **Worth narrating only where the record of effects cannot say it.** A note beside
    /// `indicate` restating the same thing adds nothing a reader could not already see and can
    /// drift from it. What no trace can hold is a decision that produced *no* effect — a message
    /// refused, a timestamp already passed, an announcement not made — and that is the case whose
    /// absence has cost this project the most.
    ///
    /// Uncallable for a protocol whose `Note` is uninhabited: there is no value to pass. The
    /// absence is checked rather than trusted, exactly as it is for a scope event.
    ///
    /// ```compile_fail
    /// # use recon_core::{Cx, NodeId, ProtoCx, Protocol, TimerId};
    /// struct Quiet;
    /// impl Protocol for Quiet {
    ///     type Cmd = ();
    ///     type Ind = ();
    ///     type Msg = ();
    ///     type Scope = core::convert::Infallible;
    ///     // Narrates nothing, and so cannot.
    ///     type Note = core::convert::Infallible;
    ///     type Meta = core::convert::Infallible;
    ///     type Entry = core::convert::Infallible;
    ///     fn on_cmd(&mut self, (): (), cx: &mut ProtoCx<'_, Self>) {
    ///         cx.note(());
    ///     }
    ///     fn on_msg(&mut self, _: NodeId, (): (), _: &mut ProtoCx<'_, Self>) {}
    ///     fn on_timer(&mut self, _: TimerId, _: &mut ProtoCx<'_, Self>) {}
    /// }
    /// ```
    pub fn note(&mut self, note: N) {
        self.notes.note(note);
    }

    /// This protocol's durable state. A write does not return until it would survive a crash, so
    /// a protocol may record a promise and then make it. See [`crate::store`].
    pub fn storage(&mut self) -> &mut dyn Store<Me, En> {
        &mut *self.store
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
    /// The mappers are normally enum variant constructors, so a wrong one is a type error.
    ///
    /// The child is handed a store it cannot write to, so only a child keeping nothing durably can
    /// be composed. See [`crate::store::NoStore`].
    /// Nothing is buffered: a child's effect is re-wrapped as it is emitted and passed straight
    /// on to this context's sink.
    pub fn with_child<CM, CI>(
        &mut self,
        msg: fn(CM) -> M,
        ind: fn(CI) -> I,
        f: impl FnOnce(&mut Cx<'_, CM, CI, N, Infallible, Infallible>),
    ) {
        let mut mapped = MapSink { parent: &mut *self.sink, msg, ind };
        let mut none = NoStore;
        let mut child = Cx {
            sink: &mut mapped,
            now: self.now,
            rng: &mut *self.rng,
            store: &mut none,
            next_timer: &mut *self.next_timer,
            notes: &mut *self.notes,
        };
        f(&mut child);
    }

    /// Run `f` against a child's context, forwarding what it sends and schedules but collecting
    /// its indications into `collected` for this protocol to handle itself.
    ///
    /// This is the usual shape. A child's indication is the parent's input — the stubborn link
    /// reporting a delivery is what the perfect link deduplicates — so it must be consumed, not
    /// passed through. `collected` belongs to the caller and is reused across events.
    pub fn with_child_consuming<CM, CI>(
        &mut self,
        msg: fn(CM) -> M,
        collected: &mut Vec<CI>,
        f: impl FnOnce(&mut Cx<'_, CM, CI, N, Infallible, Infallible>),
    ) {
        let mut sink = ConsumeSink { parent: &mut *self.sink, collected, msg };
        let mut none = NoStore;
        let mut child = Cx {
            sink: &mut sink,
            now: self.now,
            rng: &mut *self.rng,
            store: &mut none,
            next_timer: &mut *self.next_timer,
            notes: &mut *self.notes,
        };
        f(&mut child);
    }

    /// [`Cx::with_child_consuming`], for a child that keeps durable state of its own.
    ///
    /// The child is handed a view of `slot` — the part of this protocol's record that belongs to
    /// it. Its `get` projects, and its `set` reads this record back, replaces the child's part, and
    /// writes the whole thing down again. **That is one write, not two**, which is what stops a
    /// crash landing between a parent's record and its child's.
    ///
    /// Prefer [`Cx::with_child_consuming`] wherever the child keeps nothing: it hands a
    /// [`NoStore`], and a child that cannot write is one fewer thing to reason about. This exists
    /// because Algorithm 5.10 needs it — a protocol that keeps `(ets, ℓ, decision)` of its own and
    /// composes two children that each keep a record too, and whose recovery reads its children's
    /// records by name.
    ///
    /// The child's `Entry` is uninhabited: a child that *appends* cannot be composed. [`Slot`]
    /// documents why, and what the sequence half would look like if something needed it.
    pub fn with_durable_child_consuming<CM, CI, CMe>(
        &mut self,
        msg: fn(CM) -> M,
        collected: &mut Vec<CI>,
        slot: Slot<Me, CMe>,
        f: impl FnOnce(&mut Cx<'_, CM, CI, N, CMe, Infallible>),
    ) {
        let mut sink = ConsumeSink { parent: &mut *self.sink, collected, msg };
        let mut store = SlotStore { parent: &mut *self.store, slot };
        let mut child = Cx {
            sink: &mut sink,
            now: self.now,
            rng: &mut *self.rng,
            store: &mut store,
            next_timer: &mut *self.next_timer,
            notes: &mut *self.notes,
        };
        f(&mut child);
    }
}

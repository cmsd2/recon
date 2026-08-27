//! Verifies the protocol-core contract with the smallest protocols that exercise it.

use core::convert::Infallible;
use core::time::Duration;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use recon_core::{
    Cx, Effect, EffectSink, Event, MemStore, NoStore, NodeId, Position, ProtoCx, Protocol, Store,
    Time, TimerId, step, step_in, step_with,
};

const A: NodeId = NodeId::new(1);
const B: NodeId = NodeId::new(2);

fn rng(seed: u64) -> ChaCha8Rng {
    ChaCha8Rng::seed_from_u64(seed)
}

// ---------------------------------------------------------------- Echo: task 2.1

/// The smallest protocol that uses every part of the trait.
#[derive(Default)]
struct Echo {
    seen: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Ping(u32);
#[derive(Debug, Clone, PartialEq, Eq)]
struct Echoed(u32);

impl Protocol for Echo {
    type Cmd = Ping;
    type Ind = Echoed;
    type Msg = Ping;
    type Scope = core::convert::Infallible;
    /// Keeps nothing durably: a crash loses everything this protocol knows.
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    fn on_cmd(&mut self, Ping(n): Ping, cx: &mut ProtoCx<'_, Self>) {
        cx.send(B, Ping(n));
        cx.set_timer(Duration::from_millis(10));
    }

    fn on_msg(&mut self, _from: NodeId, Ping(n): Ping, cx: &mut ProtoCx<'_, Self>) {
        self.seen += 1;
        cx.indicate(Echoed(n));
    }

    fn on_timer(&mut self, _: TimerId, cx: &mut ProtoCx<'_, Self>) {
        cx.indicate(Echoed(u32::MAX));
    }
}

#[test]
fn a_trivial_protocol_compiles_against_the_trait() {
    let mut p = Echo::default();
    let mut r = rng(1);

    let fx = step(&mut p, Event::Cmd(Ping(7)), Time::ZERO, &mut r);
    assert_eq!(
        fx,
        vec![
            Effect::Send { to: B, msg: Ping(7) },
            Effect::SetTimer { after: Duration::from_millis(10), id: TimerId(0) },
        ]
    );

    let fx = step(&mut p, Event::Msg { from: A, msg: Ping(7) }, Time::ZERO, &mut r);
    assert_eq!(fx, vec![Effect::Indicate(Echoed(7))]);
    assert_eq!(p.seen, 1);

    let fx = step(&mut p, Event::Timer(TimerId(0)), Time::ZERO, &mut r);
    assert_eq!(fx, vec![Effect::Indicate(Echoed(u32::MAX))]);
}

#[test]
fn the_step_helper_reads_as_an_assertion() {
    // Task 2.3: effects are asserted directly, with no runtime and no context construction.
    let mut p = Echo::default();
    assert_eq!(
        step(&mut p, Event::Msg { from: A, msg: Ping(3) }, Time::ZERO, &mut rng(0)),
        [Effect::Indicate(Echoed(3))]
    );
}

// ------------------------------------------------- Randomness and time: task 2.2

/// Picks a peer at random and reports the time it was told.
#[derive(Default)]
struct Chooser;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Choose;
#[derive(Debug, Clone, PartialEq, Eq)]
struct Chose(u32, Time);

impl Protocol for Chooser {
    type Cmd = Choose;
    type Ind = Chose;
    type Msg = ();
    type Scope = core::convert::Infallible;
    /// Keeps nothing durably: a crash loses everything this protocol knows.
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    fn on_cmd(&mut self, Choose: Choose, cx: &mut ProtoCx<'_, Self>) {
        let pick: u32 = cx.rng().random_range(0..1_000_000);
        let now = cx.now();
        cx.indicate(Chose(pick, now));
    }
    fn on_msg(&mut self, _: NodeId, _: (), _: &mut ProtoCx<'_, Self>) {}
    fn on_timer(&mut self, _: TimerId, _: &mut ProtoCx<'_, Self>) {}
}

#[test]
fn the_same_seed_makes_the_same_choice() {
    let run = |seed: u64| {
        let mut p = Chooser;
        step(&mut p, Event::Cmd(Choose), Time::from_millis(5), &mut rng(seed))
    };
    assert_eq!(run(42), run(42), "same seed must reproduce the same choice");
}

#[test]
fn different_seeds_may_choose_differently() {
    let run = |seed: u64| {
        let mut p = Chooser;
        step(&mut p, Event::Cmd(Choose), Time::ZERO, &mut rng(seed))
    };
    // Not a guarantee for any particular pair, but over this range it must vary somewhere.
    let all_same = (0..16).all(|s| run(s) == run(0));
    assert!(!all_same, "a seeded source that ignores its seed is not a source");
}

#[test]
fn time_is_supplied_not_read() {
    let mut p = Chooser;
    let fx = step(&mut p, Event::Cmd(Choose), Time::from_millis(1234), &mut rng(0));
    match &fx[0] {
        Effect::Indicate(Chose(_, t)) => assert_eq!(*t, Time::from_millis(1234)),
        other => panic!("unexpected effect: {other:?}"),
    }
}

#[test]
fn identical_event_sequences_produce_identical_effects() {
    // The determinism requirement of protocol-core, stated directly.
    let drive = || {
        let mut p = Echo::default();
        let mut r = rng(9);
        let mut all = Vec::new();
        all.extend(step(&mut p, Event::Cmd(Ping(1)), Time::ZERO, &mut r));
        all.extend(step(
            &mut p,
            Event::Msg { from: A, msg: Ping(2) },
            Time::from_millis(1),
            &mut r,
        ));
        all.extend(step(&mut p, Event::Timer(TimerId(0)), Time::from_millis(2), &mut r));
        (all, p.seen)
    };
    assert_eq!(drive(), drive());
}

// ------------------------------------------------------- Composition: task 2.4

/// A parent that owns `Echo` and re-wraps everything it emits.
///
/// Note what is absent: no scratch buffer field, no `mem::take`, no drain loop. Each handler
/// is one call.
struct Wrapper {
    child: Echo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum WrapMsg {
    Inner(Ping),
}
#[derive(Debug, Clone, PartialEq, Eq)]
enum WrapInd {
    FromChild(Echoed),
}

impl Protocol for Wrapper {
    type Cmd = Ping;
    type Ind = WrapInd;
    type Msg = WrapMsg;
    type Scope = core::convert::Infallible;
    /// Keeps nothing durably: a crash loses everything this protocol knows.
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    fn on_cmd(&mut self, cmd: Ping, cx: &mut ProtoCx<'_, Self>) {
        let child = &mut self.child;
        cx.with_child(WrapMsg::Inner, WrapInd::FromChild, |ccx| child.on_cmd(cmd, ccx));
    }

    fn on_msg(&mut self, from: NodeId, WrapMsg::Inner(inner): WrapMsg, cx: &mut ProtoCx<'_, Self>) {
        let child = &mut self.child;
        cx.with_child(WrapMsg::Inner, WrapInd::FromChild, |ccx| child.on_msg(from, inner, ccx));
    }

    fn on_timer(&mut self, id: TimerId, cx: &mut ProtoCx<'_, Self>) {
        let child = &mut self.child;
        cx.with_child(WrapMsg::Inner, WrapInd::FromChild, |ccx| child.on_timer(id, ccx));
    }
}

#[test]
fn child_effects_surface_re_wrapped() {
    let mut w = Wrapper { child: Echo::default() };
    let mut r = rng(0);

    // A composition, so the identities must run on across calls rather than restart at each.
    let mut ids = 0;
    let mut st = MemStore::default();

    let fx = step_with(&mut w, Event::Cmd(Ping(5)), Time::ZERO, &mut r, &mut st, &mut ids);
    let registered = match fx.as_slice() {
        [Effect::Send { to: B, msg: WrapMsg::Inner(Ping(5)) }, Effect::SetTimer { after, id }]
            if *after == Duration::from_millis(10) =>
        {
            *id
        }
        other => panic!("the child's message wrapped, its timer untouched: {other:?}"),
    };

    let ev = Event::Msg { from: A, msg: WrapMsg::Inner(Ping(5)) };
    let fx = step_with(&mut w, ev, Time::ZERO, &mut r, &mut st, &mut ids);
    assert_eq!(fx, vec![Effect::Indicate(WrapInd::FromChild(Echoed(5)))]);

    // The handle the child was given comes back to it unchanged: the parent supplied no mapping,
    // so there is nothing for it to unwrap.
    let ev = Event::Timer(registered);
    let fx = step_with(&mut w, ev, Time::ZERO, &mut r, &mut st, &mut ids);
    assert_eq!(fx, vec![Effect::Indicate(WrapInd::FromChild(Echoed(u32::MAX)))]);
}

/// Two levels of nesting, to confirm mapping composes rather than only working once.
struct Outer {
    inner: Wrapper,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum OuterMsg {
    Down(WrapMsg),
}
#[derive(Debug, Clone, PartialEq, Eq)]
enum OuterInd {
    Up(WrapInd),
}

impl Protocol for Outer {
    type Cmd = Ping;
    type Ind = OuterInd;
    type Msg = OuterMsg;
    type Scope = core::convert::Infallible;
    /// Keeps nothing durably: a crash loses everything this protocol knows.
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    fn on_cmd(&mut self, cmd: Ping, cx: &mut ProtoCx<'_, Self>) {
        let inner = &mut self.inner;
        cx.with_child(OuterMsg::Down, OuterInd::Up, |ccx| inner.on_cmd(cmd, ccx));
    }
    fn on_msg(&mut self, from: NodeId, OuterMsg::Down(m): OuterMsg, cx: &mut ProtoCx<'_, Self>) {
        let inner = &mut self.inner;
        cx.with_child(OuterMsg::Down, OuterInd::Up, |ccx| inner.on_msg(from, m, ccx));
    }
    fn on_timer(&mut self, id: TimerId, cx: &mut ProtoCx<'_, Self>) {
        let inner = &mut self.inner;
        cx.with_child(OuterMsg::Down, OuterInd::Up, |ccx| inner.on_timer(id, ccx));
    }
}

#[test]
fn mapping_composes_through_two_layers() {
    let mut o = Outer { inner: Wrapper { child: Echo::default() } };
    let mut ids = 0;
    let mut st = MemStore::default();
    let fx = step_with(&mut o, Event::Cmd(Ping(2)), Time::ZERO, &mut rng(0), &mut st, &mut ids);
    // Messages nest twice over. The timer nests not at all: inserting a layer leaves the timers
    // beneath it exactly as they were, which is the point of naming one by a handle.
    assert_eq!(
        fx,
        vec![
            Effect::Send { to: B, msg: OuterMsg::Down(WrapMsg::Inner(Ping(2))) },
            Effect::SetTimer { after: Duration::from_millis(10), id: TimerId(0) },
        ]
    );
}

// ------------------------------------------- The core imposes no allocation policy

/// A sink that allocates nothing at all — the shape a `no_std` driver would use.
#[derive(Default)]
struct CountingSink {
    sends: usize,
    indications: usize,
    timers: usize,
}

impl<M, I> EffectSink<M, I> for CountingSink {
    fn emit(&mut self, effect: Effect<M, I>) {
        match effect {
            Effect::Send { .. } => self.sends += 1,
            Effect::Indicate(_) => self.indications += 1,
            Effect::SetTimer { .. } => self.timers += 1,
        }
    }
}

#[test]
fn a_protocol_runs_against_a_non_allocating_sink() {
    // Same protocol, same code path, no Vec anywhere.
    let mut sink = CountingSink::default();
    let mut r = rng(0);
    {
        let mut none = NoStore;
        let mut ids = 0;
        let mut cx = Cx::new(&mut sink, Time::ZERO, &mut r, &mut none, &mut ids);
        let mut p = Echo::default();
        p.on_cmd(Ping(1), &mut cx);
        p.on_msg(A, Ping(1), &mut cx);
    }
    assert_eq!((sink.sends, sink.indications, sink.timers), (1, 1, 1));
}

#[test]
fn composition_works_against_a_non_allocating_sink() {
    // The mapping adapter must not depend on the parent's sink being a Vec.
    let mut sink = CountingSink::default();
    let mut r = rng(0);
    {
        let mut none = NoStore;
        let mut ids = 0;
        let mut cx = Cx::new(&mut sink, Time::ZERO, &mut r, &mut none, &mut ids);
        let mut w = Wrapper { child: Echo::default() };
        w.on_cmd(Ping(1), &mut cx);
    }
    assert_eq!((sink.sends, sink.timers), (1, 1));
}

#[test]
fn a_reused_buffer_settles_its_capacity() {
    // The ordinary driver case: one Vec, reused, so allocation is amortised to nothing.
    let mut buf: Vec<Effect<WrapMsg, WrapInd>> = Vec::new();
    let mut r = rng(0);
    let mut w = Wrapper { child: Echo::default() };

    for _ in 0..4 {
        buf.clear();
        let mut none = NoStore;
        let mut ids = 0;
        let mut cx = Cx::new(&mut buf, Time::ZERO, &mut r, &mut none, &mut ids);
        w.on_cmd(Ping(1), &mut cx);
    }
    let settled = buf.capacity();
    for _ in 0..200 {
        buf.clear();
        let mut none = NoStore;
        let mut ids = 0;
        let mut cx = Cx::new(&mut buf, Time::ZERO, &mut r, &mut none, &mut ids);
        w.on_cmd(Ping(1), &mut cx);
    }
    assert_eq!(buf.capacity(), settled, "a reused buffer must not regrow per event");
}

// ------------------------------------------------------------ Effect::map is total

#[test]
fn mapping_preserves_effect_shape() {
    let e: Effect<u8, u8> = Effect::Send { to: A, msg: 1 };
    assert_eq!(e.map(|m| m + 1, |i| i), Effect::Send { to: A, msg: 2 });

    let e: Effect<u8, u8> = Effect::Indicate(1);
    assert_eq!(e.map(|m| m, |i| i + 1), Effect::Indicate(2));

    // A timer passes through untouched: there is nothing in it belonging to one layer, so there
    // is no mapper for it to be given.
    let e: Effect<u8, u8> = Effect::SetTimer { after: Duration::ZERO, id: TimerId(1) };
    assert_eq!(e.map(|m| m, |i| i), Effect::SetTimer { after: Duration::ZERO, id: TimerId(1) });
}

// ------------------------------- Scopes: tasks 1.1 to 1.3

/// A protocol whose guarantee lapses when a named condition ends.
#[derive(Default)]
struct Scoped {
    lapses: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WindowClosed(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Lapsed(u32);

impl Protocol for Scoped {
    type Cmd = ();
    type Ind = Lapsed;
    type Msg = ();
    type Scope = WindowClosed;
    /// Keeps nothing durably: a crash loses everything this protocol knows.
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    fn on_cmd(&mut self, _: (), _: &mut ProtoCx<'_, Self>) {}
    fn on_msg(&mut self, _: NodeId, _: (), _: &mut ProtoCx<'_, Self>) {}
    fn on_timer(&mut self, _: TimerId, _: &mut ProtoCx<'_, Self>) {}

    fn on_scope_event(&mut self, WindowClosed(n): WindowClosed, cx: &mut ProtoCx<'_, Self>) {
        self.lapses += 1;
        cx.indicate(Lapsed(n));
    }
}

#[test]
fn a_protocol_with_a_scope_handles_its_ending() {
    let mut p = Scoped::default();
    let fx = step(&mut p, Event::ScopeEvent(WindowClosed(7)), Time::ZERO, &mut rng(0));
    assert_eq!(fx, vec![Effect::Indicate(Lapsed(7))]);
    assert_eq!(p.lapses, 1);
}

/// A protocol that keeps something durably: a counter that survives a crash.
///
/// Deliberately tiny. What it exercises is the shape — store the whole durable value, get it back
/// on recovery, and be able to emit effects while recovering.
#[derive(Debug, Default)]
struct Counter {
    total: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Total(u32);

impl Protocol for Counter {
    type Cmd = u32;
    type Ind = Total;
    type Msg = ();
    type Scope = Infallible;
    /// Rewritten each time; a running total does not accumulate.
    type Meta = Total;
    /// One entry per bump, appended.
    type Entry = u32;

    fn on_cmd(&mut self, add: u32, cx: &mut ProtoCx<'_, Self>) {
        self.total += add;
        // Durable before announced, and one append rather than a rewrite of every bump so far.
        cx.storage().append(add);
        cx.storage().set(Total(self.total));
        cx.indicate(Total(self.total));
    }

    fn on_msg(&mut self, _from: NodeId, _msg: (), _cx: &mut ProtoCx<'_, Self>) {}
    fn on_timer(&mut self, _: TimerId, _cx: &mut ProtoCx<'_, Self>) {}

    fn on_init(&mut self, cx: &mut ProtoCx<'_, Self>) {
        // First-start-only: a later restart then recovers rather than beginning again.
        cx.storage().set(Total(self.total));
    }

    fn on_recovery(&mut self, cx: &mut ProtoCx<'_, Self>) {
        // Read, not handed over. Recovering emits an effect, which is why it is an event.
        self.total = cx.storage().get().map(|Total(t)| *t).unwrap_or(0);
        cx.indicate(Total(self.total));
    }
}

#[test]
fn a_protocol_with_no_durable_state_cannot_write() {
    // `Echo` declares both durable types uninhabited, so `set` and `append` take an argument that
    // cannot be constructed. Checked by the compiler, not trusted.
    fn _absurd_meta(m: <Echo as Protocol>::Meta) -> ! {
        match m {}
    }
    fn _absurd_entry(e: <Echo as Protocol>::Entry) -> ! {
        match e {}
    }

    // Reading stays callable and finds nothing, which is harmless.
    let mut st: MemStore<Infallible, Infallible> = MemStore::default();
    assert!(st.get().is_none());
    let mut p = Echo::default();
    let fx =
        step_in(&mut p, Event::Msg { from: A, msg: Ping(1) }, Time::ZERO, &mut rng(0), &mut st);
    assert_eq!(fx, vec![Effect::Indicate(Echoed(1))]);
}

#[test]
fn a_write_is_durable_when_it_returns() {
    let mut st = MemStore::default();
    let mut p = Counter::default();
    step_in(&mut p, Event::Cmd(3), Time::ZERO, &mut rng(0), &mut st);

    assert_eq!(st.get(), Some(&Total(3)), "readable at once, and already durable");
    assert_eq!(st.len(), 1, "one append, not a rewrite of everything so far");

    step_in(&mut p, Event::Cmd(4), Time::ZERO, &mut rng(0), &mut st);
    assert_eq!(st.get(), Some(&Total(7)));
    assert_eq!(st.len(), 2, "still one append per bump");
}

#[test]
fn a_protocol_reads_back_what_it_wrote() {
    let mut st = MemStore::default();
    let mut p = Counter::default();
    for n in 1..=3 {
        step_in(&mut p, Event::Cmd(n), Time::ZERO, &mut rng(0), &mut st);
    }
    let all: Vec<u32> = st.read_from(Position::START).into_iter().copied().collect();
    assert_eq!(all, vec![1, 2, 3]);
    assert_eq!(st.read_from(Position(1)).len(), 2, "a suffix, from a recorded position");
    assert_eq!(st.end(), Position(3));
}

#[test]
fn a_recovered_protocol_reads_what_survived_and_may_act_on_it() {
    let mut st = MemStore::default();
    st.set(Total(7));

    // A fresh instance, as a crash would produce: volatile state empty.
    let mut p = Counter::default();
    assert_eq!(p.total, 0);

    let fx = step_in(&mut p, Event::Recovery, Time::ZERO, &mut rng(0), &mut st);
    assert_eq!(p.total, 7, "read from the store, not handed over");
    assert_eq!(fx, vec![Effect::Indicate(Total(7))], "and recovering emitted an effect");
}

#[test]
fn exactly_one_of_init_and_recovery_runs_and_init_can_write() {
    // A first start must be able to do what a restart must not: writing an initial value down.
    // Repeating it on recovery would overwrite what was just read.
    let mut fresh = MemStore::default();
    step_in(&mut Counter::default(), Event::Init, Time::ZERO, &mut rng(0), &mut fresh);
    assert_eq!(fresh.get(), Some(&Total(0)), "the initial write has somewhere to happen");

    let mut existing = MemStore::default();
    existing.set(Total(9));
    let mut p = Counter::default();
    step_in(&mut p, Event::Recovery, Time::ZERO, &mut rng(0), &mut existing);
    assert_eq!(existing.get(), Some(&Total(9)), "recovering did not write over what it read");
    assert_eq!(p.total, 9);
}

#[test]
fn a_first_start_is_distinguishable_from_a_recovery() {
    let mut fresh = MemStore::default();
    assert!(fresh.is_empty(), "nothing written: this is a first start");
    step_in(&mut Counter::default(), Event::Init, Time::ZERO, &mut rng(0), &mut fresh);
    assert!(!fresh.is_empty(), "and afterwards a restart would recover");
}

#[test]
fn a_protocol_with_no_scopes_cannot_be_given_an_ending() {
    // `Echo` declares `type Scope = Infallible`, and an uninhabited type has no values — so
    // `Event::ScopeEvent(..)` cannot be constructed for it. The absence is checked by the compiler
    // rather than trusted, which is what a `#[allow]` or a runtime panic would not give.
    //
    // The nearest expressible statement is that any such value would be absurd:
    fn _absurd(s: <Echo as Protocol>::Scope) -> ! {
        match s {}
    }

    // And that the default handler, being unreachable, leaves the protocol untouched.
    let mut p = Echo::default();
    assert_eq!(p.seen, 0);
    let fx = step(&mut p, Event::Msg { from: A, msg: Ping(1) }, Time::ZERO, &mut rng(0));
    assert_eq!(fx, vec![Effect::Indicate(Echoed(1))]);
}

/// A parent that *bridges*: it handles its child's scope ending and restores the guarantee, so
/// the layer above hears nothing about it.
struct Bridger {
    child: Scoped,
    repaired: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BridgerScope {
    Child(WindowClosed),
}

impl Protocol for Bridger {
    type Cmd = ();
    type Ind = ();
    type Msg = ();
    type Scope = BridgerScope;
    /// Keeps nothing durably: a crash loses everything this protocol knows.
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    fn on_cmd(&mut self, _: (), _: &mut ProtoCx<'_, Self>) {}
    fn on_msg(&mut self, _: NodeId, _: (), _: &mut ProtoCx<'_, Self>) {}
    fn on_timer(&mut self, _: TimerId, _: &mut ProtoCx<'_, Self>) {}

    fn on_scope_event(&mut self, BridgerScope::Child(w): BridgerScope, cx: &mut ProtoCx<'_, Self>) {
        // Route down. The child's indications are consumed, not forwarded — this parent repairs
        // the lapse itself and says nothing upward.
        let child = &mut self.child;
        let mut inbox: Vec<Lapsed> = Vec::new();
        cx.with_child_consuming(|_: ()| (), &mut inbox, |ccx| child.on_scope_event(w, ccx));
        self.repaired += inbox.len() as u32;
    }
}

/// A parent that *propagates*: it cannot restore the guarantee, so it reports one of its own.
struct Propagator {
    child: Scoped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PropagatorScope {
    Child(WindowClosed),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MyGuaranteeLapsed(u32);

impl Protocol for Propagator {
    type Cmd = ();
    type Ind = MyGuaranteeLapsed;
    type Msg = ();
    type Scope = PropagatorScope;
    /// Keeps nothing durably: a crash loses everything this protocol knows.
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    fn on_cmd(&mut self, _: (), _: &mut ProtoCx<'_, Self>) {}
    fn on_msg(&mut self, _: NodeId, _: (), _: &mut ProtoCx<'_, Self>) {}
    fn on_timer(&mut self, _: TimerId, _: &mut ProtoCx<'_, Self>) {}

    fn on_scope_event(
        &mut self,
        PropagatorScope::Child(w): PropagatorScope,
        cx: &mut ProtoCx<'_, Self>,
    ) {
        let child = &mut self.child;
        let mut inbox: Vec<Lapsed> = Vec::new();
        cx.with_child_consuming(|_: ()| (), &mut inbox, |ccx| child.on_scope_event(w, ccx));
        // Re-stated in this layer's own terms rather than forwarded verbatim.
        for Lapsed(n) in inbox {
            cx.indicate(MyGuaranteeLapsed(n));
        }
    }
}

#[test]
fn a_parent_that_bridges_absorbs_the_ending() {
    let mut p = Bridger { child: Scoped::default(), repaired: 0 };
    let fx = step(
        &mut p,
        Event::ScopeEvent(BridgerScope::Child(WindowClosed(3))),
        Time::ZERO,
        &mut rng(0),
    );
    assert_eq!(fx, vec![], "a parent that repairs the lapse says nothing upward");
    assert_eq!(p.repaired, 1, "but it did see it");
    assert_eq!(p.child.lapses, 1);
}

#[test]
fn a_parent_that_cannot_bridge_propagates_in_its_own_terms() {
    let mut p = Propagator { child: Scoped::default() };
    let fx = step(
        &mut p,
        Event::ScopeEvent(PropagatorScope::Child(WindowClosed(3))),
        Time::ZERO,
        &mut rng(0),
    );
    assert_eq!(
        fx,
        vec![Effect::Indicate(MyGuaranteeLapsed(3))],
        "reported as this layer's own lapse, not as the child's"
    );
}

#[test]
fn scope_endings_route_downward_like_messages() {
    // The routing shape: a parent matches its own scope enum and calls the child, exactly as
    // on_msg and on_timer do. No new composition primitive was needed for the downward path.
    let mut p = Propagator { child: Scoped::default() };
    for i in 0..3 {
        step(
            &mut p,
            Event::ScopeEvent(PropagatorScope::Child(WindowClosed(i))),
            Time::ZERO,
            &mut rng(0),
        );
    }
    assert_eq!(p.child.lapses, 3);
}

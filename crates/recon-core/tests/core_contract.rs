//! Verifies the protocol-core contract with the smallest protocols that exercise it.

use core::time::Duration;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use recon_core::{Cx, Effect, EffectSink, Event, NodeId, ProtoCx, Protocol, Time, step};

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
#[derive(Debug, Clone, PartialEq, Eq)]
struct Tick;

impl Protocol for Echo {
    type Cmd = Ping;
    type Ind = Echoed;
    type Msg = Ping;
    type Timer = Tick;
    type Scope = core::convert::Infallible;

    fn on_cmd(&mut self, Ping(n): Ping, cx: &mut ProtoCx<'_, Self>) {
        cx.send(B, Ping(n));
        cx.set_timer(Duration::from_millis(10), Tick);
    }

    fn on_msg(&mut self, _from: NodeId, Ping(n): Ping, cx: &mut ProtoCx<'_, Self>) {
        self.seen += 1;
        cx.indicate(Echoed(n));
    }

    fn on_timer(&mut self, Tick: Tick, cx: &mut ProtoCx<'_, Self>) {
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
            Effect::SetTimer { after: Duration::from_millis(10), token: Tick },
        ]
    );

    let fx = step(&mut p, Event::Msg { from: A, msg: Ping(7) }, Time::ZERO, &mut r);
    assert_eq!(fx, vec![Effect::Indicate(Echoed(7))]);
    assert_eq!(p.seen, 1);

    let fx = step(&mut p, Event::Timer(Tick), Time::ZERO, &mut r);
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
    type Timer = ();
    type Scope = core::convert::Infallible;

    fn on_cmd(&mut self, Choose: Choose, cx: &mut ProtoCx<'_, Self>) {
        let pick: u32 = cx.rng().random_range(0..1_000_000);
        let now = cx.now();
        cx.indicate(Chose(pick, now));
    }
    fn on_msg(&mut self, _: NodeId, _: (), _: &mut ProtoCx<'_, Self>) {}
    fn on_timer(&mut self, _: (), _: &mut ProtoCx<'_, Self>) {}
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
        all.extend(step(&mut p, Event::Timer(Tick), Time::from_millis(2), &mut r));
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
#[derive(Debug, Clone, PartialEq, Eq)]
enum WrapTimer {
    Inner(Tick),
}

impl Protocol for Wrapper {
    type Cmd = Ping;
    type Ind = WrapInd;
    type Msg = WrapMsg;
    type Timer = WrapTimer;
    type Scope = core::convert::Infallible;

    fn on_cmd(&mut self, cmd: Ping, cx: &mut ProtoCx<'_, Self>) {
        let child = &mut self.child;
        cx.with_child(WrapMsg::Inner, WrapInd::FromChild, WrapTimer::Inner, |ccx| {
            child.on_cmd(cmd, ccx)
        });
    }

    fn on_msg(&mut self, from: NodeId, WrapMsg::Inner(inner): WrapMsg, cx: &mut ProtoCx<'_, Self>) {
        let child = &mut self.child;
        cx.with_child(WrapMsg::Inner, WrapInd::FromChild, WrapTimer::Inner, |ccx| {
            child.on_msg(from, inner, ccx)
        });
    }

    fn on_timer(&mut self, WrapTimer::Inner(inner): WrapTimer, cx: &mut ProtoCx<'_, Self>) {
        let child = &mut self.child;
        cx.with_child(WrapMsg::Inner, WrapInd::FromChild, WrapTimer::Inner, |ccx| {
            child.on_timer(inner, ccx)
        });
    }
}

#[test]
fn child_effects_surface_re_wrapped() {
    let mut w = Wrapper { child: Echo::default() };
    let mut r = rng(0);

    let fx = step(&mut w, Event::Cmd(Ping(5)), Time::ZERO, &mut r);
    assert_eq!(
        fx,
        vec![
            Effect::Send { to: B, msg: WrapMsg::Inner(Ping(5)) },
            Effect::SetTimer { after: Duration::from_millis(10), token: WrapTimer::Inner(Tick) },
        ],
        "the child's message and timer must arrive wrapped in the parent's types"
    );

    let fx = step(&mut w, Event::Msg { from: A, msg: WrapMsg::Inner(Ping(5)) }, Time::ZERO, &mut r);
    assert_eq!(fx, vec![Effect::Indicate(WrapInd::FromChild(Echoed(5)))]);

    let fx = step(&mut w, Event::Timer(WrapTimer::Inner(Tick)), Time::ZERO, &mut r);
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
#[derive(Debug, Clone, PartialEq, Eq)]
enum OuterTimer {
    Down(WrapTimer),
}

impl Protocol for Outer {
    type Cmd = Ping;
    type Ind = OuterInd;
    type Msg = OuterMsg;
    type Timer = OuterTimer;
    type Scope = core::convert::Infallible;

    fn on_cmd(&mut self, cmd: Ping, cx: &mut ProtoCx<'_, Self>) {
        let inner = &mut self.inner;
        cx.with_child(OuterMsg::Down, OuterInd::Up, OuterTimer::Down, |ccx| inner.on_cmd(cmd, ccx));
    }
    fn on_msg(&mut self, from: NodeId, OuterMsg::Down(m): OuterMsg, cx: &mut ProtoCx<'_, Self>) {
        let inner = &mut self.inner;
        cx.with_child(OuterMsg::Down, OuterInd::Up, OuterTimer::Down, |ccx| {
            inner.on_msg(from, m, ccx)
        });
    }
    fn on_timer(&mut self, OuterTimer::Down(t): OuterTimer, cx: &mut ProtoCx<'_, Self>) {
        let inner = &mut self.inner;
        cx.with_child(OuterMsg::Down, OuterInd::Up, OuterTimer::Down, |ccx| inner.on_timer(t, ccx));
    }
}

#[test]
fn mapping_composes_through_two_layers() {
    let mut o = Outer { inner: Wrapper { child: Echo::default() } };
    let fx = step(&mut o, Event::Cmd(Ping(2)), Time::ZERO, &mut rng(0));
    assert_eq!(
        fx,
        vec![
            Effect::Send { to: B, msg: OuterMsg::Down(WrapMsg::Inner(Ping(2))) },
            Effect::SetTimer {
                after: Duration::from_millis(10),
                token: OuterTimer::Down(WrapTimer::Inner(Tick))
            },
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

impl<M, I, T> EffectSink<M, I, T> for CountingSink {
    fn emit(&mut self, effect: Effect<M, I, T>) {
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
        let mut cx = Cx::new(&mut sink, Time::ZERO, &mut r);
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
        let mut cx = Cx::new(&mut sink, Time::ZERO, &mut r);
        let mut w = Wrapper { child: Echo::default() };
        w.on_cmd(Ping(1), &mut cx);
    }
    assert_eq!((sink.sends, sink.timers), (1, 1));
}

#[test]
fn a_reused_buffer_settles_its_capacity() {
    // The ordinary driver case: one Vec, reused, so allocation is amortised to nothing.
    let mut buf: Vec<Effect<WrapMsg, WrapInd, WrapTimer>> = Vec::new();
    let mut r = rng(0);
    let mut w = Wrapper { child: Echo::default() };

    for _ in 0..4 {
        buf.clear();
        let mut cx = Cx::new(&mut buf, Time::ZERO, &mut r);
        w.on_cmd(Ping(1), &mut cx);
    }
    let settled = buf.capacity();
    for _ in 0..200 {
        buf.clear();
        let mut cx = Cx::new(&mut buf, Time::ZERO, &mut r);
        w.on_cmd(Ping(1), &mut cx);
    }
    assert_eq!(buf.capacity(), settled, "a reused buffer must not regrow per event");
}

// ------------------------------------------------------------ Effect::map is total

#[test]
fn mapping_preserves_effect_shape() {
    let e: Effect<u8, u8, u8> = Effect::Send { to: A, msg: 1 };
    assert_eq!(e.map(|m| m + 1, |i| i, |t| t), Effect::Send { to: A, msg: 2 });

    let e: Effect<u8, u8, u8> = Effect::Indicate(1);
    assert_eq!(e.map(|m| m, |i| i + 1, |t| t), Effect::Indicate(2));

    let e: Effect<u8, u8, u8> = Effect::SetTimer { after: Duration::ZERO, token: 1 };
    assert_eq!(
        e.map(|m| m, |i| i, |t| t + 1),
        Effect::SetTimer { after: Duration::ZERO, token: 2 }
    );
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
    type Timer = ();
    type Scope = WindowClosed;

    fn on_cmd(&mut self, _: (), _: &mut ProtoCx<'_, Self>) {}
    fn on_msg(&mut self, _: NodeId, _: (), _: &mut ProtoCx<'_, Self>) {}
    fn on_timer(&mut self, _: (), _: &mut ProtoCx<'_, Self>) {}

    fn on_scope_end(&mut self, WindowClosed(n): WindowClosed, cx: &mut ProtoCx<'_, Self>) {
        self.lapses += 1;
        cx.indicate(Lapsed(n));
    }
}

#[test]
fn a_protocol_with_a_scope_handles_its_ending() {
    let mut p = Scoped::default();
    let fx = step(&mut p, Event::ScopeEnd(WindowClosed(7)), Time::ZERO, &mut rng(0));
    assert_eq!(fx, vec![Effect::Indicate(Lapsed(7))]);
    assert_eq!(p.lapses, 1);
}

#[test]
fn a_protocol_with_no_scopes_cannot_be_given_an_ending() {
    // `Echo` declares `type Scope = Infallible`, and an uninhabited type has no values — so
    // `Event::ScopeEnd(..)` cannot be constructed for it. The absence is checked by the compiler
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
    type Timer = ();
    type Scope = BridgerScope;

    fn on_cmd(&mut self, _: (), _: &mut ProtoCx<'_, Self>) {}
    fn on_msg(&mut self, _: NodeId, _: (), _: &mut ProtoCx<'_, Self>) {}
    fn on_timer(&mut self, _: (), _: &mut ProtoCx<'_, Self>) {}

    fn on_scope_end(&mut self, BridgerScope::Child(w): BridgerScope, cx: &mut ProtoCx<'_, Self>) {
        // Route down. The child's indications are consumed, not forwarded — this parent repairs
        // the lapse itself and says nothing upward.
        let child = &mut self.child;
        let mut inbox: Vec<Lapsed> = Vec::new();
        cx.with_child_consuming(
            |_: ()| (),
            |_: ()| (),
            &mut inbox,
            |ccx| child.on_scope_end(w, ccx),
        );
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
    type Timer = ();
    type Scope = PropagatorScope;

    fn on_cmd(&mut self, _: (), _: &mut ProtoCx<'_, Self>) {}
    fn on_msg(&mut self, _: NodeId, _: (), _: &mut ProtoCx<'_, Self>) {}
    fn on_timer(&mut self, _: (), _: &mut ProtoCx<'_, Self>) {}

    fn on_scope_end(
        &mut self,
        PropagatorScope::Child(w): PropagatorScope,
        cx: &mut ProtoCx<'_, Self>,
    ) {
        let child = &mut self.child;
        let mut inbox: Vec<Lapsed> = Vec::new();
        cx.with_child_consuming(
            |_: ()| (),
            |_: ()| (),
            &mut inbox,
            |ccx| child.on_scope_end(w, ccx),
        );
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
        Event::ScopeEnd(BridgerScope::Child(WindowClosed(3))),
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
        Event::ScopeEnd(PropagatorScope::Child(WindowClosed(3))),
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
            Event::ScopeEnd(PropagatorScope::Child(WindowClosed(i))),
            Time::ZERO,
            &mut rng(0),
        );
    }
    assert_eq!(p.child.lapses, 3);
}

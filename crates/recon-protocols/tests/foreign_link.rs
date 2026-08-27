//! That a link this project never wrote can carry a protocol this project did.
//!
//! The scenario is an application with its own transport — a driver, a shared-memory ring, a
//! socket it manages itself — that wants the broadcasts above it without forking them. What it has
//! to satisfy is `Link<P>`, and nothing else.

use core::time::Duration;
use recon_core::{NodeId, ProtoCx, Protocol, TimerId};
use recon_protocols::best_effort_broadcast::{BestEffortBroadcast, Cmd, Ind};
use recon_protocols::perfect_link as pl;
use recon_sim::{Config, Sim};

const A: NodeId = NodeId::new(1);
const B: NodeId = NodeId::new(2);
const C: NodeId = NodeId::new(3);
const ALL: [NodeId; 3] = [A, B, C];

/// Somebody else's link: no retransmission, no deduplication, no timer. It speaks the link port
/// and nothing more, which is the whole of what a broadcast asks of it.
#[derive(Debug)]
struct DriverLink<P>(core::marker::PhantomData<fn() -> P>);

impl<P> Default for DriverLink<P> {
    fn default() -> Self {
        DriverLink(core::marker::PhantomData)
    }
}

impl<P: Clone> Protocol for DriverLink<P> {
    type Cmd = pl::Cmd<P>;
    type Ind = pl::Ind<P>;
    type Msg = P;
    type Scope = core::convert::Infallible;
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    fn on_cmd(&mut self, pl::Cmd::Send { to, msg }: Self::Cmd, cx: &mut ProtoCx<'_, Self>) {
        cx.send(to, msg);
    }

    fn on_msg(&mut self, from: NodeId, msg: P, cx: &mut ProtoCx<'_, Self>) {
        cx.indicate(pl::Ind::Deliver { from, msg });
    }

    fn on_timer(&mut self, _id: TimerId, _cx: &mut ProtoCx<'_, Self>) {
        // Registers none, and has no child to pass one to.
    }
}

type Beb = BestEffortBroadcast<u32, DriverLink<u32>>;

#[test]
fn a_broadcast_runs_over_a_link_the_library_never_wrote() {
    let mut s: Sim<Beb> = Sim::new(Config::default().seed(1), &ALL, |me| {
        BestEffortBroadcast::with_link(me, ALL, DriverLink::default())
    });
    s.command(A, Cmd::Broadcast(7));
    s.run_for(Duration::from_millis(200));

    for n in ALL {
        let got: Vec<u32> =
            s.trace().indications_at(n).map(|Ind::Deliver { msg, .. }| *msg).collect();
        assert_eq!(got, vec![7], "{n} delivered over the foreign link");
    }
}

#[test]
fn the_foreign_link_really_is_a_different_stack() {
    // Non-vacuity: the library's own link puts an identifier on the wire, this one puts the
    // payload straight on it. If the broadcast were secretly using the built-in link, the wire
    // type would not be `u32`.
    let mut s: Sim<Beb> = Sim::new(Config::default().seed(2), &ALL, |me| {
        BestEffortBroadcast::with_link(me, ALL, DriverLink::default())
    });
    s.command(A, Cmd::Broadcast(9));
    s.run_for(Duration::from_millis(200));

    let sent: Vec<u32> = s.trace().sends().map(|(_, _, m)| *m).collect();
    assert_eq!(sent.len(), ALL.len(), "one send per peer, and the wire is the bare payload");
    assert!(sent.iter().all(|m| *m == 9));
}

// ------------------------------- and the consensus protocol, on the same foreign link

/// The scenario in full: somebody else's link, this library's consensus, neither edited.
#[test]
fn consensus_runs_over_a_link_the_library_never_wrote() {
    use recon_protocols::flooding_consensus::{
        Cmd as FcCmd, Flood, FloodingConsensus, Ind as FcInd,
    };

    const BOUND: Duration = Duration::from_millis(20);
    type Fc = FloodingConsensus<u32, DriverLink<Flood<u32>>>;

    let mut s: Sim<Fc> = Sim::new(Config::default().seed(1).synchronous(BOUND), &ALL, |me| {
        FloodingConsensus::with_link(me, ALL, DriverLink::default(), BOUND * 2, BOUND * 6)
    });
    for (n, v) in ALL.iter().zip([7u32, 8, 9]) {
        s.command(*n, FcCmd::Propose(v));
    }
    s.run_for(Duration::from_millis(2000));

    let decisions: Vec<u32> = ALL
        .iter()
        .map(|n| {
            s.trace()
                .indications_at(*n)
                .map(|FcInd::Decide(v)| *v)
                .next()
                .unwrap_or_else(|| panic!("{n} decided nothing"))
        })
        .collect();
    assert!(decisions.windows(2).all(|w| w[0] == w[1]), "agreement: {decisions:?}");
    assert!(decisions.iter().all(|d| [7, 8, 9].contains(d)), "and it was proposed");
}

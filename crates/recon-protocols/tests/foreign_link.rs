//! That a link this project never wrote can carry a protocol this project did.
//!
//! The scenario is an application with its own transport — a driver, a shared-memory ring, a
//! socket it manages itself — that wants the broadcasts above it without forking them. What it has
//! to satisfy is `Link<P>`, and nothing else.

use core::time::Duration;
use recon_core::{NodeId, ProtoCx, Protocol, TimerId};
use recon_protocols::best_effort_broadcast::{BestEffortBroadcast, Cmd, Ind};
use recon_protocols::link::{Link, LinkInd};
use recon_sim::{Config, Sim};

const A: NodeId = NodeId::new(1);
const B: NodeId = NodeId::new(2);
const C: NodeId = NodeId::new(3);
const ALL: [NodeId; 3] = [A, B, C];

/// What this link is asked to do, in its own words rather than another link's.
///
/// The port carries the translations, not the types, so a foreign link is free to name its
/// requests and reports whatever it likes. Deliberately not the perfect link's types: reusing
/// its vocabulary would make this a demonstration that the perfect link's shape works, which is
/// not the claim. This file now imports no link at all.
#[derive(Debug, Clone, PartialEq, Eq)]
enum DriverCmd<P> {
    Transmit { peer: NodeId, bytes: P },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DriverInd<P> {
    Arrived { peer: NodeId, bytes: P },
}

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
    type Cmd = DriverCmd<P>;
    type Ind = DriverInd<P>;
    type Msg = P;
    type Scope = core::convert::Infallible;
    type Note = recon_protocols::Note;
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    fn on_cmd(
        &mut self,
        DriverCmd::Transmit { peer, bytes }: Self::Cmd,
        cx: &mut ProtoCx<'_, Self>,
    ) {
        cx.send(peer, bytes);
    }

    fn on_msg(&mut self, from: NodeId, msg: P, cx: &mut ProtoCx<'_, Self>) {
        cx.indicate(DriverInd::Arrived { peer: from, bytes: msg });
    }

    fn on_timer(&mut self, _id: TimerId, _cx: &mut ProtoCx<'_, Self>) {
        // Registers none, and has no child to pass one to.
    }
}

/// Satisfying the port is what makes this link usable, and it is two functions: how to ask for a
/// send, and what an indication means. Nothing about the broadcast above appears here, and nothing
/// about this link appears there.
///
/// It does not implement `ScopedLink`. It has no session and no epoch, so it cannot observe a
/// boundary, and a layer needing one cannot be composed over it — which is the honest outcome
/// rather than a stack that waits for a re-establishment nobody will report.
impl<P: Clone> Link<P> for DriverLink<P> {
    fn send(to: NodeId, msg: P) -> DriverCmd<P> {
        DriverCmd::Transmit { peer: to, bytes: msg }
    }

    fn classify(DriverInd::Arrived { peer, bytes }: DriverInd<P>) -> LinkInd<P> {
        LinkInd::Deliver { from: peer, msg: bytes }
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
        let got: Vec<u32> = s
            .trace()
            .indications_at(n)
            .filter_map(|ind| match ind {
                Ind::Deliver { msg, .. } => Some(*msg),
                _ => None,
            })
            .collect();
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

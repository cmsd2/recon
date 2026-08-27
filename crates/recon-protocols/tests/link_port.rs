//! The link port: that it admits both links, that the scoped claim is honest, and that a link
//! failing to satisfy it is rejected when the project is built rather than when a run misbehaves.

use recon_core::{NodeId, ProtoCx, Protocol, TimerId};
use recon_protocols::link::{Boundary, Link, LinkInd, ScopedLink};
use recon_protocols::perfect_link::{self as pl, PerfectLink};
use recon_protocols::session_link::{self as sl, SessionLink};

const A: NodeId = NodeId::new(1);
const B: NodeId = NodeId::new(2);

// -------------------------------------------------- Both links satisfy the port

#[test]
fn both_links_satisfy_the_port_despite_differing_vocabularies() {
    // The whole reason the port carries translations rather than pinning types: `pl::Ind` has one
    // variant and `sl::Ind` has three, and a bound naming either one excludes the other. That
    // exclusion is what forked four broadcast modules.
    fn accepts<P, L: Link<P>>() {}
    accepts::<u32, PerfectLink<u32>>();
    accepts::<u32, SessionLink<u32>>();
}

#[test]
fn the_port_builds_each_links_own_request() {
    // A layer above never constructs a request itself, because the request is the link's
    // vocabulary. It asks the port for one.
    assert_eq!(<PerfectLink<u32> as Link<u32>>::send(B, 7), pl::Cmd::Send { to: B, msg: 7 });
    assert_eq!(<SessionLink<u32> as Link<u32>>::send(B, 7), sl::Cmd::Send { to: B, msg: 7 });
}

#[test]
fn a_delivery_classifies_the_same_way_through_either_link() {
    // The point of the port: one layer above, reading one vocabulary, over two implementations.
    let expected = LinkInd::Deliver { from: A, msg: 7u32 };
    assert_eq!(
        <PerfectLink<u32> as Link<u32>>::classify(pl::Ind::Deliver { from: A, msg: 7 }),
        expected
    );
    assert_eq!(
        <SessionLink<u32> as Link<u32>>::classify(sl::Ind::Deliver { from: A, msg: 7 }),
        expected
    );
}

// ---------------------------------------- The scoped claim, and that it is honest

#[test]
fn the_session_link_claims_to_report_boundaries_and_does() {
    // `ScopedLink` carries no methods, so what makes the claim honest is this: every indication the
    // session link raises that is not a delivery classifies as a boundary, naming the peer and the
    // epoch. A link implementing the trait without doing so would satisfy the compiler and lie.
    fn requires_scoped<P, L: ScopedLink<P>>() {}
    requires_scoped::<u32, SessionLink<u32>>();

    assert_eq!(
        <SessionLink<u32> as Link<u32>>::classify(sl::Ind::SessionEnded { peer: B, epoch: 3 }),
        LinkInd::Boundary(Boundary::Ended { peer: B, epoch: 3 })
    );
    assert_eq!(
        <SessionLink<u32> as Link<u32>>::classify(sl::Ind::SessionEstablished {
            peer: B,
            epoch: 4
        }),
        LinkInd::Boundary(Boundary::Established { peer: B, epoch: 4 })
    );
}

#[test]
fn the_perfect_link_reports_no_boundary_and_does_not_claim_to() {
    // It cannot observe one — PL2 is scoped to the recipient's incarnation and the link has no
    // means of seeing that incarnation end — so it must not say it can.
    // `docs/scope-annotated-modules.md` forbids a module declaring a scope it cannot observe.
    //
    // That `PerfectLink` does not implement `ScopedLink` is enforced by the compiler, and pinned by
    // a `compile_fail` doctest on `ScopedLink` itself — doctests run only for library targets, so it
    // has to live there rather than here. What this test adds is the behavioural half: its
    // classification never yields one.
    let every_indication = [pl::Ind::Deliver { from: A, msg: 1u32 }];
    for ind in every_indication {
        assert!(
            !matches!(<PerfectLink<u32> as Link<u32>>::classify(ind), LinkInd::Boundary(_)),
            "a perfect link must never report a boundary it cannot observe"
        );
    }
}

// ----------------------------------- A link that does not satisfy the port is rejected

/// Speaks a vocabulary of its own and never implements [`Link`].
///
/// It is a perfectly good protocol — that is the point. Being a protocol is not being a link, and
/// the port is what says so.
struct NotALink;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Whatever;

impl Protocol for NotALink {
    type Cmd = Whatever;
    type Ind = Whatever;
    type Msg = Whatever;
    type Scope = core::convert::Infallible;
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    fn on_cmd(&mut self, _: Whatever, _: &mut ProtoCx<'_, Self>) {}
    fn on_msg(&mut self, _: NodeId, _: Whatever, _: &mut ProtoCx<'_, Self>) {}
    fn on_timer(&mut self, _: TimerId, _: &mut ProtoCx<'_, Self>) {}
}

#[test]
fn a_protocol_is_not_a_link_merely_by_being_a_protocol() {
    // The earlier exploratory port was a blanket impl over every `Protocol` with the right
    // associated types, which made satisfying it an accident of shape rather than a decision. This
    // asserts the opposite: `NotALink` implements `Protocol` in full and is still not a link.
    //
    // The negative is stated by the compiler, in the `compile_fail` doctest on `Link`. This test
    // pins the positive half so that the pair cannot both rot: `NotALink` really is a protocol.
    fn is_a_protocol<T: Protocol>() {}
    is_a_protocol::<NotALink>();
}

//! The detector port: that it admits both detectors, that the perfect one's honesty about never
//! retracting is a property of its indications rather than a claim, and that a protocol which is
//! not a detector is rejected when the project is built.

use core::time::Duration;
use recon_core::NodeId;
use recon_protocols::detector::{Detector, DetectorInd};
use recon_protocols::eventually_perfect_failure_detector::{
    self as dp, EventuallyPerfectFailureDetector,
};
use recon_protocols::perfect_failure_detector::{self as pfd, PerfectFailureDetector};

const A: NodeId = NodeId::new(1);
const B: NodeId = NodeId::new(2);

#[test]
fn both_detectors_satisfy_the_port_despite_differing_vocabularies() {
    // The reason the port translates rather than pinning types: `pfd::Ind` has one variant and
    // `dp::Ind` has two, and a bound naming either excludes the other — which would leave Ω
    // unable to compose over both, the situation four forked broadcast modules came from.
    fn accepts<D: Detector>() {}
    accepts::<PerfectFailureDetector>();
    accepts::<EventuallyPerfectFailureDetector>();
}

#[test]
fn the_perfect_detector_yields_a_suspicion_and_has_no_withdrawal_to_yield() {
    assert_eq!(
        PerfectFailureDetector::classify(pfd::Ind::Crash { node: B }),
        DetectorInd::Suspect { node: B }
    );
    // Exhaustive over `pfd::Ind`: adding a variant that could become a withdrawal would fail to
    // compile here, which is the claim "never retracts" made checkable rather than asserted.
    fn _every_variant_is_a_suspicion(ind: pfd::Ind) -> DetectorInd {
        match ind {
            pfd::Ind::Crash { node } => DetectorInd::Suspect { node },
        }
    }
}

#[test]
fn the_eventually_perfect_detector_yields_both() {
    assert_eq!(
        EventuallyPerfectFailureDetector::classify(dp::Ind::Suspect { node: B }),
        DetectorInd::Suspect { node: B }
    );
    assert_eq!(
        EventuallyPerfectFailureDetector::classify(dp::Ind::Restore { node: B }),
        DetectorInd::Restore { node: B }
    );
}

#[test]
fn a_detector_keeps_nothing_durable_the_layer_above_must_know_about() {
    use recon_protocols::detector::VolatileDetector;
    fn accepts<D: VolatileDetector>() {}
    accepts::<PerfectFailureDetector>();
    accepts::<EventuallyPerfectFailureDetector>();
}

#[test]
fn the_two_detectors_are_configured_in_their_own_terms() {
    // Not a behaviour claim — a note that the port deliberately says nothing about configuration.
    // `P` takes a fixed timeout because its accuracy rests on one; `◇P` takes a range because its
    // whole subject is moving within it.
    let p = PerfectFailureDetector::new(
        A,
        [A, B],
        Duration::from_millis(10),
        Duration::from_millis(40),
    );
    assert_eq!(p.timeout(), Duration::from_millis(40));

    let dp = EventuallyPerfectFailureDetector::new(
        A,
        [A, B],
        dp::Config::new(
            Duration::from_millis(10),
            Duration::from_millis(40),
            Duration::from_millis(400),
        ),
    );
    assert_eq!(dp.delay(), Duration::from_millis(40), "starts at the initial delay");
}

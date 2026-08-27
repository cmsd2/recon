//! TEMPORARY allocation probe. Not part of the suite; delete after reading.

use core::sync::atomic::{AtomicUsize, Ordering};
use core::time::Duration;
use recon_core::NodeId;
use recon_sim::{Config, Sim};
use std::alloc::{GlobalAlloc, Layout, System};

static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);
static SMALL: AtomicUsize = AtomicUsize::new(0);

struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(l.size(), Ordering::Relaxed);
        if (65..=256).contains(&l.size()) {
            SMALL.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
}

#[global_allocator]
static A: Counting = Counting;

const ALL: [NodeId; 5] =
    [NodeId::new(1), NodeId::new(2), NodeId::new(3), NodeId::new(4), NodeId::new(5)];

fn probe<P: recon_core::Protocol>(
    label: &str,
    n: usize,
    mut build: impl FnMut() -> Sim<P>,
    mut drive: impl FnMut(&mut Sim<P>, u32),
) where
    P::Msg: Clone + PartialEq,
    P::Ind: Clone,
    P::Meta: Clone,
    P::Entry: Clone,
{
    let mut s = build();
    for i in 0..n {
        drive(&mut s, i as u32);
    }
    s.run_for(Duration::from_millis(2000));
    let deliveries = s.trace().deliveries().count();

    let (a0, b0, s0) = (
        ALLOCS.load(Ordering::Relaxed),
        BYTES.load(Ordering::Relaxed),
        SMALL.load(Ordering::Relaxed),
    );
    let mut s2 = build();
    for i in 0..n {
        drive(&mut s2, i as u32);
    }
    s2.run_for(Duration::from_millis(2000));
    let (a1, b1, s1) = (
        ALLOCS.load(Ordering::Relaxed),
        BYTES.load(Ordering::Relaxed),
        SMALL.load(Ordering::Relaxed),
    );

    let (allocs, bytes, small) = (a1 - a0, b1 - b0, s1 - s0);
    println!(
        "PROBE {label}: {deliveries} deliveries | {allocs} allocs ({small} 65-256B) | {bytes} bytes \
         | {:.1} allocs/delivery",
        allocs as f64 / deliveries.max(1) as f64
    );
}

#[test]
fn measure() {
    use recon_protocols::uniform_reliable_broadcast as urb;
    probe(
        "urb",
        50,
        || {
            Sim::new(Config::default().seed(1).synchronous(Duration::from_millis(20)), &ALL, |me| {
                urb::UniformReliableBroadcast::<u32>::new(
                    me,
                    ALL,
                    Duration::from_millis(10),
                    Duration::from_millis(40),
                    Duration::from_millis(200),
                )
            })
        },
        |s, i| s.command(ALL[0], urb::Cmd::Broadcast(i)),
    );

    use recon_protocols::logged_uniform_reliable_broadcast as lurb;
    probe(
        "logged_urb",
        50,
        || {
            Sim::new(Config::default().seed(1), &ALL, |me| {
                lurb::LoggedUniformReliableBroadcast::<u32>::new(me, ALL, Duration::from_millis(10))
            })
        },
        |s, i| s.command(ALL[0], lurb::Cmd::Broadcast(i)),
    );
}

#[test]
fn sizes() {
    use recon_protocols::best_effort_broadcast as beb;
    use recon_protocols::uniform_reliable_broadcast::Data;
    println!("SIZE beb::Ind<Data<u32>> = {}", core::mem::size_of::<beb::Ind<Data<u32>>>());
    println!(
        "SIZE pfd::Ind = {}",
        core::mem::size_of::<recon_protocols::perfect_failure_detector::Ind>()
    );
}

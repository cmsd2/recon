//! Distributed algorithms, written as sans-IO protocols.
//!
//! Each module is one abstraction from Cachin, Guerraoui & Rodrigues, *Introduction to
//! Reliable and Secure Distributed Programming* — transcribed so that the code can be read
//! against the page. The pseudocode each one implements is quoted in its module documentation.
//!
//! The bottom abstraction, fair-loss links, is not here: it is what the simulator provides.

pub mod best_effort_broadcast;
pub mod flooding_consensus;
pub mod link;
pub mod logged_link;
pub mod logged_uniform_reliable_broadcast;
pub mod majority_ack_uniform_reliable_broadcast;
pub mod perfect_failure_detector;
pub mod perfect_link;
pub mod reliable_broadcast;
pub mod session_link;
pub mod stubborn_broadcast;
pub mod stubborn_link;
pub mod uniform_reliable_broadcast;

pub use best_effort_broadcast::BestEffortBroadcast;
pub use flooding_consensus::FloodingConsensus;
pub use logged_link::LoggedLink;
pub use logged_uniform_reliable_broadcast::LoggedUniformReliableBroadcast;
pub use majority_ack_uniform_reliable_broadcast::MajorityAckUniformReliableBroadcast;
pub use perfect_failure_detector::PerfectFailureDetector;
pub use perfect_link::PerfectLink;
pub use reliable_broadcast::ReliableBroadcast;
pub use session_link::SessionLink;
pub use stubborn_broadcast::StubbornBroadcast;
pub use stubborn_link::StubbornLink;
pub use uniform_reliable_broadcast::UniformReliableBroadcast;

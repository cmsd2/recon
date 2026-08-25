//! A deterministic simulator for sans-IO protocols.
//!
//! The simulator is the deliverable, not a test harness. It provides the fair-loss network the
//! bottom of the protocol ladder assumes, a virtual clock, and a seeded generator, so that a
//! run is completely determined by its seed and configuration — and a failing run is a number
//! you can replay.

pub mod codec;
pub mod config;
pub mod sim;
pub mod trace;

pub use config::Config;
pub use sim::Sim;
pub use trace::{DropReason, Trace, TraceEvent};

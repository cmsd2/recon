//! Core abstractions for sans-IO distributed protocols.
//!
//! A protocol here is a synchronous state machine: it consumes events and emits effects.
//! It never awaits, never reads a clock, never draws ambient randomness, and never performs
//! input or output. Everything it needs from the world arrives through [`Cx`], which is what
//! makes a run reproducible from a seed and a protocol testable as a plain function.
//!
//! Layers compose by ownership: a parent holds its child as a typed field and re-wraps the
//! child's effects into its own terms via [`Cx::with_child`]. There is no registry, no string
//! key, and no lookup performed while running — a mis-wired stack fails to compile.

pub mod child;
pub mod cx;
pub mod effect;
pub mod error;
pub mod node;
pub mod protocol;
pub mod session;
pub mod store;
pub mod time;

pub use child::Child;
pub use cx::{Cx, EffectSink};
pub use effect::{Effect, TimerId, WriteKind};
pub use node::NodeId;
pub use protocol::{Event, ProtoCx, ProtoEffect, ProtoEvent, Protocol, step, step_in, step_with};
pub use session::SessionEvent;
pub use store::{MemStore, NoStore, Position, Slot, Store};
pub use time::Time;

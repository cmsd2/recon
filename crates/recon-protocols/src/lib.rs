//! Distributed algorithms, written as sans-IO protocols.
//!
//! Each module is a rung of the ladder in Cachin, Guerraoui & Rodrigues, *Introduction to
//! Reliable and Secure Distributed Programming* — transcribed so that the code can be read
//! against the page. The pseudocode each one implements is quoted in its module documentation.
//!
//! The bottom rung, fair-loss links, is not here: it is what the simulator provides.

pub mod perfect_link;
pub mod stubborn_link;

pub use perfect_link::PerfectLink;
pub use stubborn_link::StubbornLink;

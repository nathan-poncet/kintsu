//! The innermost ring: what a command line is, what it returned, what
//! deserves attention. Pure, synchronous, deterministic: no I/O, no clock,
//! no terminal. Everything else depends on this; this depends on nothing
//! else in the crate.

pub mod command;
pub mod exit_status;
pub mod outcome;
pub mod shell;
pub mod triage;

pub use command::{CommandLine, CommandLineError};
pub use exit_status::ExitStatus;
pub use outcome::CommandOutcome;
pub use shell::Shell;
pub use triage::{QuietReason, TriageDecision};

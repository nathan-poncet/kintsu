//! The innermost ring of Kintsu: what a command line is, what it returned,
//! what deserves attention. Pure, synchronous, deterministic: no I/O, no
//! clock, no terminal. Everything of ours depends on this; this depends on
//! nothing of ours.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

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

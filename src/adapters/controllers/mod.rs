//! Inbound adapters: from the outside world to a use case call.

pub mod cli;
pub mod socket;

pub use cli::{Command, DaemonAction, ScopeFlag, parse_args};
pub use socket::{Request, parse_frame};

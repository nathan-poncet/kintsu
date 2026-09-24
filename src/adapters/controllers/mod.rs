//! Inbound adapters: from the outside world to a use case call.

pub mod cli;

pub use cli::{Command, ScopeFlag, parse_args};

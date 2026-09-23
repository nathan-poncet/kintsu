//! Inbound adapters: from the outside world to a use case call.

pub mod cli;

pub use cli::{CliError, Command, parse_args};

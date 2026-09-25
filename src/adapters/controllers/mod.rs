//! Inbound adapters: from the outside world to a use case call.

pub mod cli;
pub mod setup_prompts;
pub mod socket;

pub use cli::{Command, DaemonAction, ScopeFlag, parse_args};
pub use setup_prompts::Prompter;
pub use socket::{Request, parse_frame};

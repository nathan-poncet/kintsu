//! Inbound adapters: from the outside world to a use case call.

pub mod cli;
pub mod panel_keys;
pub mod setup_prompts;
pub mod socket;
pub mod url_scheme;

pub use cli::{Command, DaemonAction, ScopeFlag, ServiceAction, parse_args};
pub use panel_keys::key_for;
pub use setup_prompts::Prompter;
pub use socket::{Request, parse_frame};
pub use url_scheme::{act_url, parse_act_url};

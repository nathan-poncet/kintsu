//! The innermost ring: what a command line is, what it returned, what a
//! failure case contains, what a fix is and how dangerous, what must be
//! redacted, what the agent receives. Pure, synchronous, deterministic:
//! no I/O, no clock, no terminal. Everything else depends on this; this
//! depends on nothing else in the crate.

pub mod brief;
pub mod case;
pub mod command;
pub mod danger;
pub mod distance;
pub mod exit_status;
pub mod fix;
pub mod ignore;
pub mod message;
pub mod outcome;
pub mod redaction;
pub mod rules;
pub mod session;
pub mod settings;
pub mod shell;
pub mod time;
pub mod triage;

pub use brief::{CaseDocument, case_document, hand_off_brief};
pub use case::{CaseId, FailureCase};
pub use command::{CommandLine, CommandLineError};
pub use danger::{Danger, classify_danger};
pub use exit_status::ExitStatus;
pub use fix::{Confidence, Fix, FixSource};
pub use ignore::{IgnoreEntry, IgnoreScope, IgnoreTarget};
pub use message::{Message, MessageBody};
pub use outcome::CommandOutcome;
pub use redaction::{Redacted, redact};
pub use rules::{DirEntry, Facts, Os, suggest_fix};
pub use session::{Session, SessionId};
pub use settings::{
    EagerFix, KeySource, ModelSpec, Provider, QuietSettings, Routing, Settings, Tier, UiMode,
    UiSettings,
};
pub use shell::Shell;
pub use time::{Duration, Timestamp};
pub use triage::{QuietReason, TriageDecision};

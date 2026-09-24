//! The application rules: one interactor per thing a user or a hook can
//! ask for, each driving the ports it needs. As pure as the entities: no
//! I/O, no executor, no terminal. Async, when it comes, arrives as
//! `impl Future` on the ports; the runtime stays in the outer rings.

pub mod capture;
pub mod diagnose;
pub mod explain;
pub mod facts;
pub mod fix_last;
pub mod hand_off;
pub mod ignore;
pub mod messages;
pub mod ports;
pub mod privacy;
pub mod prompts;
pub mod routing;
#[cfg(test)]
pub mod testing;
pub mod triage;

pub use capture::CaptureOutput;
pub use diagnose::{Check, Diagnose, Health};
pub use explain::{Explain, Explanation};
pub use fix_last::{FixLast, FixProposal};
pub use hand_off::{HandOff, HandOffPlan};
pub use ignore::{Ignore, IgnoreRequest, ScopeChoice};
pub use messages::Messages;
pub use privacy::Privacy;
pub use triage::{Triage, TriageInput};

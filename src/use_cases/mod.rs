//! The application rules: one interactor per thing a user or a hook can
//! ask for, each driving the ports it needs. As pure as the entities: no
//! I/O, no executor, no terminal. Async, when it comes, arrives as
//! `impl Future` on the ports; the runtime stays in the outer rings.

pub mod capture;
pub mod costs;
pub mod diagnose;
pub mod explain;
pub mod facts;
pub mod fix_last;
pub mod focus;
pub mod hand_off;
pub mod ignore;
pub mod learned;
pub mod login;
pub mod messages;
pub mod models;
pub mod ports;
pub mod privacy;
pub mod prompts;
pub mod routing;
pub mod setup;
pub mod stats;
#[cfg(test)]
pub mod testing;
pub mod triage;

pub use capture::CaptureOutput;
pub use costs::{CostReport, Costs};
pub use diagnose::{Check, Diagnose, Health};
pub use explain::{Explain, Explained};
pub use fix_last::{FixLast, FixProposal};
pub use focus::Focus;
pub use hand_off::{HandOff, HandOffPlan};
pub use ignore::{Ignore, IgnoreRequest, ScopeChoice};
pub use learned::{Forget, Learned};
pub use login::{LoggedIn, Login};
pub use messages::Messages;
pub use models::{KeyStatus, ListModels, ModelRow, Probe, ProbeModels, Reach};
pub use privacy::Privacy;
pub use setup::{Detect, compose};
pub use stats::{Stats, StatsReport};
pub use triage::{Triage, TriageInput};

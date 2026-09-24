//! What Kintsu decided about a finished command line.

use crate::entities::{FailureCase, Fix};

/// The outcome of triage: say nothing, or offer help.
#[derive(Debug, Clone, PartialEq)]
pub enum TriageDecision {
    /// Nothing is shown.
    Quiet(QuietReason),
    /// The command failed in a way worth a bubble, with a fix when a rule knew one.
    Offer {
        /// The failure and its context.
        case: Box<FailureCase>,
        /// The instant fix, if any rule matched.
        fix: Option<Fix>,
    },
}

/// Why triage stayed quiet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuietReason {
    /// The command succeeded.
    Succeeded,
    /// The user stopped the command themselves.
    Interrupted,
    /// The status is one the user declared fine for this program, or in general.
    AcceptedStatus,
    /// The program is on the never-triage list.
    NeverTriaged(String),
    /// An ignore entry or a mute covers it.
    Ignored,
    /// The same failure was just reported.
    Duplicate,
}

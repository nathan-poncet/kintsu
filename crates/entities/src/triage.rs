//! What Kintsu decided about a finished command line.

use crate::CommandOutcome;

/// The outcome of triage: say nothing, or offer help.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriageDecision {
    /// Nothing is shown.
    Quiet(QuietReason),
    /// The command failed in a way worth a bubble.
    Offer(CommandOutcome),
}

/// Why triage stayed quiet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuietReason {
    /// The command succeeded.
    Succeeded,
    /// The user stopped the command themselves.
    Interrupted,
}

impl TriageDecision {
    /// Whether nothing will be shown.
    pub const fn is_quiet(&self) -> bool {
        matches!(self, Self::Quiet(_))
    }
}

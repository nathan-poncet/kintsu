//! What the model calls cost, one line per call, and the day's budget note.

use thiserror::Error;

use crate::entities::{Day, LedgerEntry, Timestamp};

/// Why the ledger could not be read or written.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LedgerError {
    #[error("ledger: {0}")]
    Io(String),
}

/// Keeps what every model call cost.
pub trait CostLedger {
    /// One more call.
    fn record(&self, entry: &LedgerEntry) -> Result<(), LedgerError>;

    /// The calls at or after `from`, oldest first.
    fn since(&self, from: Timestamp) -> Result<Vec<LedgerEntry>, LedgerError>;

    /// Whether the user was already told, that day, that the budget is spent.
    fn budget_noted(&self, day: Day) -> Result<bool, LedgerError>;

    /// The user was told.
    fn note_budget(&self, day: Day) -> Result<(), LedgerError>;
}

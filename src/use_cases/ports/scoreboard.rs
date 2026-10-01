//! Where what was offered and what was taken is kept: the score of the
//! rules and the models, apart from what the calls cost.

use thiserror::Error;

use crate::entities::{FixEvent, Timestamp};

/// Why the scoreboard could not be read or written.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ScoreboardError {
    #[error("scoreboard: {0}")]
    Io(String),
}

/// Keeps every failure looked at, every fix offered and every fix taken.
pub trait Scoreboard {
    /// One more event.
    fn mark(&self, event: &FixEvent) -> Result<(), ScoreboardError>;

    /// The events at or after `from`, oldest first.
    fn since(&self, from: Timestamp) -> Result<Vec<FixEvent>, ScoreboardError>;
}

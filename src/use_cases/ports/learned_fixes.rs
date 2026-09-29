//! Where the fixes the user took twice are kept.

use thiserror::Error;

use crate::entities::{CommandLine, FailureShape, LearnedFix, Timestamp};

/// Why the learned fixes could not be read or written.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LearnedFixesError {
    /// The storage is unreachable or corrupt.
    #[error("learned fixes unavailable: {0}")]
    Unavailable(String),
}

/// Keeps what repeated acceptances taught; the list rules live in
/// `LearnedBook`, every store applies them.
pub trait LearnedFixes {
    /// The fix learned for this shape of failure, if any.
    fn recall(&self, shape: &FailureShape) -> Result<Option<LearnedFix>, LearnedFixesError>;
    /// One more time this fix was taken for this shape; a different fix
    /// for the same shape starts over. Returns the entry as it stands.
    fn accept(
        &self,
        shape: &FailureShape,
        command: &CommandLine,
        at: Timestamp,
    ) -> Result<LearnedFix, LearnedFixesError>;
    /// Every entry, most recently taken first.
    fn entries(&self) -> Result<Vec<LearnedFix>, LearnedFixesError>;
    /// Forgets one program's entries, or all of them; how many went.
    fn forget(&self, program: Option<&str>) -> Result<usize, LearnedFixesError>;
}

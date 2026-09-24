//! Where the last failure of each shell is kept for `why`, `fix`, `agent`.

use thiserror::Error;

use crate::entities::{FailureCase, SessionId};

/// Why a case could not be read or written.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CaseStoreError {
    /// The storage is unreachable or corrupt.
    #[error("case storage unavailable: {0}")]
    Unavailable(String),
}

/// Remembers failure cases.
pub trait CaseStore {
    /// Saves a case; it becomes the last one of its session and overall.
    fn save(&self, case: &FailureCase) -> Result<(), CaseStoreError>;
    /// The last case of a session, or the last case at all when no
    /// session is known.
    fn last(&self, session: Option<&SessionId>) -> Result<Option<FailureCase>, CaseStoreError>;
}

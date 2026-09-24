//! Where the ignore list lives.

use thiserror::Error;

use crate::entities::IgnoreEntry;

/// Why the ignore list could not be read or written.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum IgnoreStoreError {
    /// The storage is unreachable or corrupt.
    #[error("ignore list unavailable: {0}")]
    Unavailable(String),
}

/// Keeps the ignore list.
pub trait IgnoreStore {
    /// Every entry, expired ones included.
    fn entries(&self) -> Result<Vec<IgnoreEntry>, IgnoreStoreError>;
    /// Replaces the whole list.
    fn replace(&self, entries: &[IgnoreEntry]) -> Result<(), IgnoreStoreError>;
}

//! Where sessions and their recent commands live between two prompts.

use thiserror::Error;

use crate::entities::{Session, SessionId};

/// Why a session could not be read or written.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RegistryError {
    /// The storage is unreachable or corrupt.
    #[error("session storage unavailable: {0}")]
    Unavailable(String),
}

/// Keeps one record per interactive shell.
pub trait SessionRegistry {
    /// The session, when it has been seen before.
    fn load(&self, id: &SessionId) -> Result<Option<Session>, RegistryError>;
    /// Writes the session, replacing what was there.
    fn save(&self, session: &Session) -> Result<(), RegistryError>;
}

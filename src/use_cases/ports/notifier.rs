//! Delivering a message to a live shell, or keeping it until the shell asks.

use thiserror::Error;

use crate::entities::{Message, SessionId};

/// Why a message could not be handed over.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NotifyError {
    /// The session is not known to the daemon.
    #[error("unknown session {0}")]
    UnknownSession(String),
}

/// Sends messages to shells.
pub trait Notifier {
    /// Queues the message for the session; a subscribed shell gets it at once.
    fn deliver(&self, session: &SessionId, message: Message) -> Result<(), NotifyError>;
}

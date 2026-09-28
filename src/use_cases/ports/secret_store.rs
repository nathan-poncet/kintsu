//! Where `kintsu login` puts a key: the OS keychain, or a fake in tests.

use thiserror::Error;

use crate::entities::SecretKey;

/// Why a key could not be stored.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SecretStoreError {
    /// No keychain to speak to on this machine.
    #[error("no keychain available: {0}")]
    Unavailable(String),
    /// The keychain refused.
    #[error("the keychain refused: {0}")]
    Refused(String),
}

/// Keeps keys under the `kintsu` service, one account per model.
pub trait SecretStore {
    /// Stores or replaces the key of `account`.
    fn store(&self, account: &str, key: &SecretKey) -> Result<(), SecretStoreError>;
}

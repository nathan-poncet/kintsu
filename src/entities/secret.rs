//! An API key on its way to the keychain: a value that never prints.

use std::fmt;

use thiserror::Error;

/// A key as the user typed it, trimmed, never empty.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretKey(String);

/// Why a piece of text is not a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SecretKeyError {
    #[error("the key is empty")]
    Empty,
}

impl SecretKey {
    pub fn new(text: impl AsRef<str>) -> Result<Self, SecretKeyError> {
        let trimmed = text.as_ref().trim();
        if trimmed.is_empty() {
            return Err(SecretKeyError::Empty);
        }
        Ok(Self(trimmed.to_string()))
    }

    /// The key itself, for the gateway that stores it and nothing else.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretKey([redacted])")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_is_trimmed_never_empty_and_never_debug_printed() {
        let key = SecretKey::new("  sk-live-1\n").unwrap();
        assert_eq!(key.expose(), "sk-live-1");
        assert_eq!(format!("{key:?}"), "SecretKey([redacted])");
        assert_eq!(SecretKey::new("   "), Err(SecretKeyError::Empty));
    }
}

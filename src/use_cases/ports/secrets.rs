//! Where keys come from, without the use cases touching the environment.

use crate::entities::KeySource;

/// Resolves a key source into a key.
pub trait Secrets {
    /// The key, when the source yields one.
    fn lookup(&self, source: &KeySource) -> Option<String>;
}

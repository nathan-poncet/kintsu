//! Unguessable identities for cases.

use crate::entities::CaseId;

/// Hands out case identities that cannot be guessed from the previous one.
pub trait IdGenerator {
    /// A fresh case id.
    fn case_id(&self) -> CaseId;
}

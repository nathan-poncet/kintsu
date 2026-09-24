//! Case ids nobody can guess from the previous one: 128 bits from the
//! process's randomised hasher, the clock and the pid.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::entities::CaseId;
use crate::use_cases::ports::IdGenerator;

pub struct RandomIds;

impl IdGenerator for RandomIds {
    fn case_id(&self) -> CaseId {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let mut token = String::with_capacity(32);
        for salt in [0u64, 1] {
            let mut h = RandomState::new().build_hasher();
            h.write_u128(nanos);
            h.write_u32(std::process::id());
            h.write_u64(salt);
            token.push_str(&format!("{:016x}", h.finish()));
        }
        CaseId::new(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_long_hex_and_never_repeat() {
        let a = RandomIds.case_id();
        let b = RandomIds.case_id();
        assert_eq!(a.as_str().len(), 32);
        assert!(a.as_str().chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }
}

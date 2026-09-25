//! Case ids nobody can guess: 128 bits from the system's randomness, or,
//! should that be unreadable, from the process's randomised hasher, the
//! clock and the pid.

use std::collections::hash_map::RandomState;
use std::fs::File;
use std::hash::{BuildHasher, Hasher};
use std::io::{self, Read};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::entities::CaseId;
use crate::use_cases::ports::IdGenerator;

pub struct RandomIds;

impl IdGenerator for RandomIds {
    fn case_id(&self) -> CaseId {
        let bytes = system_random().unwrap_or_else(|_| hashed_entropy());
        CaseId::new(bytes.iter().map(|b| format!("{b:02x}")).collect::<String>())
    }
}

fn system_random() -> io::Result<[u8; 16]> {
    let mut bytes = [0u8; 16];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn hashed_entropy() -> [u8; 16] {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut bytes = [0u8; 16];
    for (salt, half) in bytes.chunks_mut(8).enumerate() {
        let mut h = RandomState::new().build_hasher();
        h.write_u128(nanos);
        h.write_u32(std::process::id());
        h.write_usize(salt);
        half.copy_from_slice(&h.finish().to_be_bytes());
    }
    bytes
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

    #[test]
    fn the_system_is_the_source_and_the_fallback_never_repeats_either() {
        assert!(system_random().is_ok());
        assert_ne!(hashed_entropy(), hashed_entropy());
    }
}

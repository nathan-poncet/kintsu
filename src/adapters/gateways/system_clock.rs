//! The wall clock.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::entities::Timestamp;
use crate::use_cases::ports::Clock;

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        Timestamp::from_millis(u64::try_from(ms).unwrap_or(u64::MAX))
    }
}

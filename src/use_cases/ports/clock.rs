//! The current time, asked for once per use case and passed on as a value.

use crate::entities::Timestamp;

/// Tells the time.
pub trait Clock {
    /// Now, in milliseconds since the epoch.
    fn now(&self) -> Timestamp;
}

//! Time as values, never as a clock: instants and durations arrive from a
//! port and are compared here.

use std::fmt;

/// Milliseconds since the Unix epoch, as reported by a clock port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(u64);

/// A length of time in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Duration(u64);

impl Timestamp {
    /// Wraps milliseconds since the epoch.
    pub const fn from_millis(ms: u64) -> Self {
        Self(ms)
    }

    /// Milliseconds since the epoch.
    pub const fn as_millis(self) -> u64 {
        self.0
    }

    /// This instant plus a duration.
    pub const fn plus(self, d: Duration) -> Self {
        Self(self.0.saturating_add(d.0))
    }
}

impl Duration {
    /// From milliseconds.
    pub const fn from_millis(ms: u64) -> Self {
        Self(ms)
    }

    /// From whole seconds.
    pub const fn from_secs(s: u64) -> Self {
        Self(s * 1000)
    }

    /// From whole minutes.
    pub const fn from_mins(m: u64) -> Self {
        Self(m * 60_000)
    }

    /// In milliseconds.
    pub const fn as_millis(self) -> u64 {
        self.0
    }
}

impl fmt::Display for Duration {
    /// Short, as a human reads it under a command: `0.01 s`, `12 s`, `3 min`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ms = self.0;
        if ms < 1000 {
            write!(f, "{:.2} s", ms as f64 / 1000.0)
        } else if ms < 60_000 {
            write!(f, "{} s", ms / 1000)
        } else {
            write!(f, "{} min", ms / 60_000)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_read_the_way_a_person_says_them() {
        assert_eq!(Duration::from_millis(12).to_string(), "0.01 s");
        assert_eq!(Duration::from_millis(12_400).to_string(), "12 s");
        assert_eq!(Duration::from_mins(3).to_string(), "3 min");
    }

    #[test]
    fn instants_add_without_wrapping() {
        let t = Timestamp::from_millis(1_000);
        assert_eq!(
            t.plus(Duration::from_secs(2)),
            Timestamp::from_millis(3_000)
        );
        assert_eq!(
            Timestamp::from_millis(u64::MAX).plus(Duration::from_secs(1)),
            Timestamp::from_millis(u64::MAX)
        );
    }
}

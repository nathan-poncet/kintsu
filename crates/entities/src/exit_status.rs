//! What a command returned to the shell.

use std::fmt;

/// The exit status of a command line, as the shell saw it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExitStatus(i32);

impl ExitStatus {
    /// The status of a command that succeeded.
    pub const SUCCESS: Self = Self(0);

    /// Wraps a raw status code.
    pub const fn new(code: i32) -> Self {
        Self(code)
    }

    /// The raw code.
    pub const fn code(self) -> i32 {
        self.0
    }

    /// Zero.
    pub const fn is_success(self) -> bool {
        self.0 == 0
    }

    /// The user stopped the command, it did not break: Ctrl-C (SIGINT, 130),
    /// a reader that closed early such as `| head` (SIGPIPE, 141), Ctrl-Z
    /// (SIGTSTP, 148).
    pub const fn is_interruption(self) -> bool {
        matches!(self.0, 130 | 141 | 148)
    }

    /// The shell found no such program (127).
    pub const fn is_command_not_found(self) -> bool {
        self.0 == 127
    }
}

impl fmt::Display for ExitStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_is_success_and_nothing_else_is() {
        assert!(ExitStatus::SUCCESS.is_success());
        assert!(!ExitStatus::new(1).is_success());
    }

    #[test]
    fn ctrl_c_broken_pipes_and_ctrl_z_are_interruptions() {
        for code in [130, 141, 148] {
            assert!(ExitStatus::new(code).is_interruption(), "{code}");
        }
    }

    #[test]
    fn ordinary_failures_are_not_interruptions() {
        for code in [1, 2, 126, 127, 128, 137, 255] {
            assert!(!ExitStatus::new(code).is_interruption(), "{code}");
        }
    }

    #[test]
    fn one_hundred_twenty_seven_means_command_not_found() {
        assert!(ExitStatus::new(127).is_command_not_found());
        assert!(!ExitStatus::new(126).is_command_not_found());
    }
}

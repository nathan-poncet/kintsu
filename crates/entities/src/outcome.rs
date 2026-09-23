//! What the shell hook reports once a command line has finished.

use crate::{CommandLine, ExitStatus};

/// A finished command line and what it returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutcome {
    command: CommandLine,
    status: ExitStatus,
}

impl CommandOutcome {
    /// Pairs a command line with its exit status.
    pub const fn new(command: CommandLine, status: ExitStatus) -> Self {
        Self { command, status }
    }

    /// The command line as typed.
    pub const fn command(&self) -> &CommandLine {
        &self.command
    }

    /// What it returned.
    pub const fn status(&self) -> ExitStatus {
        self.status
    }
}

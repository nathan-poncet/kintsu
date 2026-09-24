//! What the shell hook reports once a command line has finished.

use crate::entities::{CommandLine, Duration, ExitStatus};

/// A finished command line, what it returned, and how long it took.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutcome {
    command: CommandLine,
    status: ExitStatus,
    duration: Option<Duration>,
}

impl CommandOutcome {
    /// Pairs a command line with its exit status; the duration is unknown.
    pub const fn new(command: CommandLine, status: ExitStatus) -> Self {
        Self {
            command,
            status,
            duration: None,
        }
    }

    /// The same outcome with how long the command took.
    pub const fn lasting(mut self, duration: Duration) -> Self {
        self.duration = Some(duration);
        self
    }

    /// The command line as typed.
    pub const fn command(&self) -> &CommandLine {
        &self.command
    }

    /// What it returned.
    pub const fn status(&self) -> ExitStatus {
        self.status
    }

    /// How long it took, when the hook measured it.
    pub const fn duration(&self) -> Option<Duration> {
        self.duration
    }

    /// The same command and status seen twice make one failure: words and
    /// code make the fingerprint; spacing and timing do not.
    pub fn fingerprint(&self) -> String {
        format!(
            "{}\u{1f}{}",
            self.command.words().join(" "),
            self.status.code()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_outcome_can_carry_its_duration() {
        let outcome = CommandOutcome::new(CommandLine::new("make").unwrap(), ExitStatus::new(2))
            .lasting(Duration::from_secs(12));
        assert_eq!(outcome.duration(), Some(Duration::from_secs(12)));
        assert_eq!(outcome.status(), ExitStatus::new(2));
    }

    #[test]
    fn the_same_command_and_status_share_a_fingerprint_whatever_the_spacing() {
        let make =
            |t: &str, c: i32| CommandOutcome::new(CommandLine::new(t).unwrap(), ExitStatus::new(c));
        assert_eq!(
            make("make  test", 2).fingerprint(),
            make("make test", 2).fingerprint()
        );
        assert_ne!(
            make("make test", 2).fingerprint(),
            make("make test", 1).fingerprint()
        );
    }
}

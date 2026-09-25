//! What the shell hook reports once a command line has finished.

use crate::entities::{CommandLine, Duration, ExitStatus};

/// A finished command line, what it returned, and how long it took. In a
/// pipeline the status is the first stage's that failed, not the last
/// stage's, and `stage` says which one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutcome {
    command: CommandLine,
    status: ExitStatus,
    duration: Option<Duration>,
    pipestatus: Vec<ExitStatus>,
    stage: Option<usize>,
}

impl CommandOutcome {
    /// Pairs a command line with its exit status; the duration is unknown.
    pub const fn new(command: CommandLine, status: ExitStatus) -> Self {
        Self {
            command,
            status,
            duration: None,
            pipestatus: Vec::new(),
            stage: None,
        }
    }

    /// The same outcome knowing every stage's status, as `$pipestatus`
    /// gives them. The first stage that failed is the failure: a typo in
    /// the first stage is a typo even when `| head` succeeded, `grep` that
    /// found nothing is still accepted under `pipefail`, and a stage that
    /// died because its reader closed early is no failure at all.
    pub fn in_pipeline(mut self, statuses: Vec<ExitStatus>) -> Self {
        if statuses.len() < 2 {
            return self;
        }
        if let Some(index) = statuses
            .iter()
            .position(|s| !s.is_success() && !s.is_interruption())
        {
            self.status = statuses[index];
            self.stage = Some(index);
        }
        self.pipestatus = statuses;
        self
    }

    /// Every stage's status, when the shell gave them.
    pub fn pipestatus(&self) -> &[ExitStatus] {
        &self.pipestatus
    }

    /// The stage of the pipeline that failed, as typed.
    pub fn failed_stage(&self) -> Option<&str> {
        let index = self.stage?;
        self.command.stages().get(index).copied()
    }

    /// The program that failed: the failed stage's in a pipeline, else
    /// the command line's first word.
    pub fn program(&self) -> &str {
        match self.failed_stage() {
            Some(stage) => stage.split_whitespace().next().unwrap_or_default(),
            None => self.command.program(),
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
    fn in_a_pipeline_the_first_stage_that_failed_is_the_failure() {
        let pipeline = |t: &str, last: i32, all: &[i32]| {
            CommandOutcome::new(CommandLine::new(t).unwrap(), ExitStatus::new(last))
                .in_pipeline(all.iter().map(|c| ExitStatus::new(*c)).collect())
        };
        let typo = pipeline("gti status | head -1", 0, &[127, 0]);
        assert_eq!(typo.status(), ExitStatus::new(127));
        assert_eq!(typo.failed_stage(), Some("gti status"));
        assert_eq!(typo.program(), "gti");
        let closed_early = pipeline("yes | head -1", 0, &[141, 0]);
        assert_eq!(closed_early.status(), ExitStatus::new(0));
        assert_eq!(closed_early.failed_stage(), None);
        let no_match = pipeline("cat log | grep x", 1, &[0, 1]);
        assert_eq!((no_match.status().code(), no_match.program()), (1, "grep"));
        let single = pipeline("make", 2, &[2]);
        assert_eq!(
            (
                single.status().code(),
                single.program(),
                single.pipestatus().len()
            ),
            (2, "make", 0)
        );
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

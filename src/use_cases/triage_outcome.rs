//! Decide whether a finished command line deserves a bubble.

use crate::entities::{CommandOutcome, QuietReason, TriageDecision};

/// Stays quiet on success and on interruptions the user caused; offers help
/// for everything else. The denylist, the cooldown and the same-failure
/// dedupe will join here once the daemon remembers sessions.
pub fn triage_outcome(outcome: CommandOutcome) -> TriageDecision {
    let status = outcome.status();
    if status.is_success() {
        return TriageDecision::Quiet(QuietReason::Succeeded);
    }
    if status.is_interruption() {
        return TriageDecision::Quiet(QuietReason::Interrupted);
    }
    TriageDecision::Offer(outcome)
}

#[cfg(test)]
mod tests {
    use crate::entities::{CommandLine, ExitStatus};

    use super::*;

    fn outcome(command: &str, code: i32) -> CommandOutcome {
        CommandOutcome::new(CommandLine::new(command).unwrap(), ExitStatus::new(code))
    }

    #[test]
    fn a_successful_command_stays_quiet() {
        assert_eq!(
            triage_outcome(outcome("ls", 0)),
            TriageDecision::Quiet(QuietReason::Succeeded)
        );
    }

    #[test]
    fn a_command_the_user_stopped_stays_quiet() {
        for code in [130, 141, 148] {
            assert_eq!(
                triage_outcome(outcome("sleep 10", code)),
                TriageDecision::Quiet(QuietReason::Interrupted),
                "{code}"
            );
        }
    }

    #[test]
    fn a_failure_is_offered_with_its_outcome_intact() {
        let failed = outcome("gti status", 127);
        assert_eq!(
            triage_outcome(failed.clone()),
            TriageDecision::Offer(failed)
        );
    }
}

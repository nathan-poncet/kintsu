//! What to read from the machine before asking the rules: as little as the
//! status allows, because this runs on the quiet path.

use crate::entities::{
    CommandOutcome, Facts, FailureCase, FailureShape, Fix, suggest_fix, suggest_fix_from_output,
};
use crate::use_cases::ports::{Environment, LearnedFixes};

/// The PATH is scanned only when the shell could not find or run the
/// program; the working directory is read whenever it is known.
pub fn gather_facts(
    environment: &dyn Environment,
    outcome: &CommandOutcome,
    cwd: Option<&str>,
) -> Facts {
    let status = outcome.status();
    let needs_path = status.is_command_not_found() || status.is_not_executable();
    Facts {
        os: environment.os(),
        executables: if needs_path {
            environment.executables()
        } else {
            Vec::new()
        },
        cwd_entries: cwd.map(|dir| environment.entries(dir)).unwrap_or_default(),
    }
}

/// What the rules know about a case: the line first, then its output
/// when the terminal gave it, then what the user taught by taking the
/// same fix twice. No model is asked here.
pub fn rule_fix(
    environment: &dyn Environment,
    learned: &dyn LearnedFixes,
    case: &FailureCase,
) -> Option<Fix> {
    let facts = gather_facts(environment, case.outcome(), case.cwd());
    suggest_fix(case.outcome(), &facts)
        .or_else(|| {
            case.output()
                .and_then(|output| suggest_fix_from_output(case.outcome(), &facts, output))
        })
        .or_else(|| learned_fix(learned, case.outcome()))
}

/// The fix the user took twice for this very failure, when the store can
/// be read: an unreadable store is no reason to say nothing else.
pub fn learned_fix(learned: &dyn LearnedFixes, outcome: &CommandOutcome) -> Option<Fix> {
    learned
        .recall(&FailureShape::of(outcome))
        .ok()
        .flatten()
        .and_then(|entry| entry.as_rule())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{CommandLine, DirEntry, ExitStatus, Os, Timestamp};
    use crate::use_cases::testing::{FakeEnvironment, MemoryLearned, case, outcome};

    #[test]
    fn the_line_is_tried_before_the_output_and_the_output_only_when_there_is_one() {
        let env = FakeEnvironment::with_executables(&["git"]);
        let learned = MemoryLearned::default();
        let typo = case("gti push", 127, None).with_output("fish: Unknown command: gti".into());
        assert_eq!(
            rule_fix(&env, &learned, &typo).unwrap().command().as_str(),
            "git push"
        );
        let refused = case("touch /etc/x", 1, None);
        assert!(
            rule_fix(&env, &learned, &refused).is_none(),
            "no output yet"
        );
        let refused = refused.with_output("touch: /etc/x: Permission denied".into());
        assert_eq!(
            rule_fix(&env, &learned, &refused)
                .unwrap()
                .command()
                .as_str(),
            "sudo touch /etc/x"
        );
    }

    #[test]
    fn a_fix_taken_twice_comes_after_the_built_in_rules_and_before_any_model() {
        let env = FakeEnvironment::with_executables(&[]);
        let learned = MemoryLearned::default();
        let shape = FailureShape::new("make test", ExitStatus::new(2));
        let fix = CommandLine::new("make -j4 test").unwrap();
        let failure = case("make test", 2, None);
        learned
            .accept(&shape, &fix, Timestamp::from_millis(1))
            .unwrap();
        assert!(
            rule_fix(&env, &learned, &failure).is_none(),
            "once is a coincidence"
        );
        learned
            .accept(&shape, &fix, Timestamp::from_millis(2))
            .unwrap();
        let rule = rule_fix(&env, &learned, &failure).unwrap();
        assert_eq!(rule.command(), &fix);
        assert_eq!(
            rule.source(),
            &crate::entities::FixSource::Rule("learned".into())
        );
        let other = case("make build", 2, None);
        assert!(
            rule_fix(&env, &learned, &other).is_none(),
            "another failure"
        );
    }

    #[test]
    fn the_path_is_scanned_only_for_a_command_not_found() {
        let env = FakeEnvironment::with_executables(&["git"]);
        let facts = gather_facts(&env, &outcome("make", 2), Some("/w"));
        assert!(facts.executables.is_empty());
        assert_eq!(env.path_reads.get(), 0);
        let facts = gather_facts(&env, &outcome("gti", 127), None);
        assert_eq!(facts.executables, vec!["git"]);
        assert_eq!(env.path_reads.get(), 1);
        assert_eq!(facts.os, Some(Os::Linux));
    }

    #[test]
    fn the_working_directory_is_read_when_known() {
        let mut env = FakeEnvironment::with_executables(&[]);
        env.entries.insert(
            "/w".into(),
            vec![DirEntry {
                name: "apps".into(),
                is_dir: true,
                is_executable: true,
            }],
        );
        assert_eq!(
            gather_facts(&env, &outcome("cd aps", 1), Some("/w"))
                .cwd_entries
                .len(),
            1
        );
        assert!(
            gather_facts(&env, &outcome("cd aps", 1), None)
                .cwd_entries
                .is_empty()
        );
    }
}

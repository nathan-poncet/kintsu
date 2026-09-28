//! What to read from the machine before asking the rules: as little as the
//! status allows, because this runs on the quiet path.

use crate::entities::{
    CommandOutcome, Facts, FailureCase, Fix, suggest_fix, suggest_fix_from_output,
};
use crate::use_cases::ports::Environment;

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
/// when the terminal gave it. No model is asked here.
pub fn rule_fix(environment: &dyn Environment, case: &FailureCase) -> Option<Fix> {
    let facts = gather_facts(environment, case.outcome(), case.cwd());
    suggest_fix(case.outcome(), &facts).or_else(|| {
        case.output()
            .and_then(|output| suggest_fix_from_output(case.outcome(), &facts, output))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{DirEntry, Os};
    use crate::use_cases::testing::{FakeEnvironment, case, outcome};

    #[test]
    fn the_line_is_tried_before_the_output_and_the_output_only_when_there_is_one() {
        let env = FakeEnvironment::with_executables(&["git"]);
        let typo = case("gti push", 127, None).with_output("fish: Unknown command: gti".into());
        assert_eq!(
            rule_fix(&env, &typo).unwrap().command().as_str(),
            "git push"
        );
        let refused = case("touch /etc/x", 1, None);
        assert!(rule_fix(&env, &refused).is_none(), "no output yet");
        let refused = refused.with_output("touch: /etc/x: Permission denied".into());
        assert_eq!(
            rule_fix(&env, &refused).unwrap().command().as_str(),
            "sudo touch /etc/x"
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

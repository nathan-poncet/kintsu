//! What to read from the machine before asking the rules: as little as the
//! status allows, because this runs on the quiet path.

use crate::entities::{CommandOutcome, Facts};
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{DirEntry, Os};
    use crate::use_cases::testing::{FakeEnvironment, outcome};

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

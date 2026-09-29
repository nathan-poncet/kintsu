//! `kintsu learned`: what repeated acceptances taught, and forgetting it.

use crate::entities::LearnedFix;
use crate::use_cases::ports::{LearnedFixes, LearnedFixesError};

/// What `kintsu learned forget` asks to unlearn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Forget {
    /// Every fix learned for this program.
    Program(String),
    /// Everything.
    All,
}

/// Lists and forgets the learned fixes.
pub struct Learned<'a> {
    pub learned: &'a dyn LearnedFixes,
}

impl Learned<'_> {
    /// Every learned fix, most recently taken first.
    pub fn entries(&self) -> Result<Vec<LearnedFix>, LearnedFixesError> {
        self.learned.entries()
    }

    /// Unlearns; how many entries went.
    pub fn forget(&self, what: &Forget) -> Result<usize, LearnedFixesError> {
        match what {
            Forget::Program(program) => self.learned.forget(Some(program)),
            Forget::All => self.learned.forget(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{CommandLine, ExitStatus, FailureShape, Timestamp};
    use crate::use_cases::testing::MemoryLearned;

    #[test]
    fn what_was_learned_is_listed_and_forgotten_by_program_or_at_once() {
        let store = MemoryLearned::default();
        for (line, fix, at) in [
            ("make test", "make -j4 test", 1),
            ("cargo publish", "cargo publish --allow-dirty", 3),
            ("make", "make all", 2),
        ] {
            store
                .accept(
                    &FailureShape::new(line, ExitStatus::new(2)),
                    &CommandLine::new(fix).unwrap(),
                    Timestamp::from_millis(at),
                )
                .unwrap();
        }
        let uc = Learned { learned: &store };
        let lines: Vec<String> = uc
            .entries()
            .unwrap()
            .iter()
            .map(|e| e.shape().line().to_string())
            .collect();
        assert_eq!(lines, ["cargo publish", "make", "make test"]);
        assert_eq!(uc.forget(&Forget::Program("make".into())).unwrap(), 2);
        assert_eq!(uc.forget(&Forget::All).unwrap(), 1);
        assert!(uc.entries().unwrap().is_empty());
    }
}

//! The learned fixes as one JSON file in the state directory. Small, read
//! whole, and on the quiet path only when the built-in rules found
//! nothing.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::entities::{CommandLine, ExitStatus, FailureShape, LearnedBook, LearnedFix, Timestamp};
use crate::use_cases::ports::{LearnedFixes, LearnedFixesError};

pub struct JsonLearnedFixes {
    path: PathBuf,
}

impl JsonLearnedFixes {
    /// Under the state directory.
    pub fn new(state_dir: impl AsRef<Path>) -> Self {
        Self {
            path: state_dir.as_ref().join("learned.json"),
        }
    }

    fn load(&self) -> Result<LearnedBook, LearnedFixesError> {
        let dtos: Vec<EntryDto> = match std::fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| self.unavailable(&e))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(self.unavailable(&e)),
        };
        Ok(LearnedBook::from_entries(
            dtos.into_iter().filter_map(EntryDto::into_entry).collect(),
        ))
    }

    fn store(&self, book: &LearnedBook) -> Result<(), LearnedFixesError> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| LearnedFixesError::Unavailable("no parent directory".into()))?;
        std::fs::create_dir_all(parent).map_err(|e| self.unavailable(&e))?;
        let dtos: Vec<EntryDto> = book.entries().iter().map(EntryDto::from).collect();
        let bytes = serde_json::to_vec(&dtos).map_err(|e| self.unavailable(&e))?;
        let tmp = self
            .path
            .with_extension(format!("tmp-{}", std::process::id()));
        std::fs::write(&tmp, bytes).map_err(|e| self.unavailable(&e))?;
        std::fs::rename(&tmp, &self.path).map_err(|e| self.unavailable(&e))
    }

    fn unavailable(&self, error: &dyn std::fmt::Display) -> LearnedFixesError {
        LearnedFixesError::Unavailable(format!("{}: {error}", self.path.display()))
    }
}

#[derive(Serialize, Deserialize)]
struct EntryDto {
    line: String,
    status: i32,
    command: String,
    acceptances: u32,
    last_accepted_ms: u64,
}

impl EntryDto {
    fn from(entry: &LearnedFix) -> Self {
        Self {
            line: entry.shape().line().to_string(),
            status: entry.shape().status().code(),
            command: entry.command().as_str().to_string(),
            acceptances: entry.acceptances(),
            last_accepted_ms: entry.last_accepted().as_millis(),
        }
    }

    fn into_entry(self) -> Option<LearnedFix> {
        Some(LearnedFix::restore(
            FailureShape::new(&self.line, ExitStatus::new(self.status)),
            CommandLine::new(self.command).ok()?,
            self.acceptances,
            Timestamp::from_millis(self.last_accepted_ms),
        ))
    }
}

impl LearnedFixes for JsonLearnedFixes {
    fn recall(&self, shape: &FailureShape) -> Result<Option<LearnedFix>, LearnedFixesError> {
        Ok(self.load()?.recall(shape).cloned())
    }

    fn accept(
        &self,
        shape: &FailureShape,
        command: &CommandLine,
        at: Timestamp,
    ) -> Result<LearnedFix, LearnedFixesError> {
        let mut book = self.load()?;
        let entry = book.accept(shape, command, at);
        self.store(&book)?;
        Ok(entry)
    }

    fn entries(&self) -> Result<Vec<LearnedFix>, LearnedFixesError> {
        Ok(self.load()?.entries())
    }

    fn forget(&self, program: Option<&str>) -> Result<usize, LearnedFixesError> {
        let mut book = self.load()?;
        let gone = book.forget(program);
        self.store(&book)?;
        Ok(gone)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::use_cases::testing::learned_fixes_contract;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("kintsu-learned-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn the_file_store_passes_the_contract() {
        learned_fixes_contract(&JsonLearnedFixes::new(scratch("contract")));
    }

    #[test]
    fn what_was_accepted_survives_a_new_process_and_a_corrupt_file_is_an_error() {
        let dir = scratch("reopen");
        let shape = FailureShape::new("make test", ExitStatus::new(2));
        let fix = CommandLine::new("make -j4 test").unwrap();
        JsonLearnedFixes::new(&dir)
            .accept(&shape, &fix, Timestamp::from_millis(1))
            .unwrap();
        let again = JsonLearnedFixes::new(&dir);
        assert_eq!(again.recall(&shape).unwrap().unwrap().acceptances(), 1);
        std::fs::write(dir.join("learned.json"), b"{not json").unwrap();
        assert!(matches!(
            again.entries(),
            Err(LearnedFixesError::Unavailable(_))
        ));
        assert!(
            JsonLearnedFixes::new(dir.join("nowhere"))
                .entries()
                .unwrap()
                .is_empty(),
            "no file yet is an empty book"
        );
    }
}

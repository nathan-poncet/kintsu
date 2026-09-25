//! Small files the shell hooks read at the next prompt, written by the
//! client right after it printed something the hook must know about.
//! The paths are a contract with `shell/kintsu.*`:
//! `<state>/sessions/<id>.asking` says an "asking…" line waits for its
//! answer; `<state>/sessions/<id>.ghost` holds a fix safe enough to be
//! pre-typed on the next prompt.

use std::io;
use std::path::{Path, PathBuf};

use crate::entities::SessionId;

pub struct HookNotes {
    dir: PathBuf,
}

impl HookNotes {
    pub fn new(state_dir: &Path) -> Self {
        Self {
            dir: state_dir.join("sessions"),
        }
    }

    pub fn asking_path(&self, session: &SessionId) -> PathBuf {
        self.dir.join(format!("{}.asking", session.as_str()))
    }

    pub fn ghost_path(&self, session: &SessionId) -> PathBuf {
        self.dir.join(format!("{}.ghost", session.as_str()))
    }

    /// An "asking…" line was printed; the answer may replace it.
    pub fn set_asking(&self, session: &SessionId) -> io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        std::fs::write(self.asking_path(session), b"")
    }

    /// A fix the shell may pre-type, dim, on the next prompt.
    pub fn set_ghost(&self, session: &SessionId, command: &str) -> io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        std::fs::write(self.ghost_path(session), command.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_notes_are_files_named_after_the_session() {
        let dir = std::env::temp_dir().join(format!("kintsu-notes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let notes = HookNotes::new(&dir);
        let id = SessionId::new("42");
        assert_eq!(
            notes.asking_path(&id),
            dir.join("sessions").join("42.asking")
        );
        assert_eq!(notes.ghost_path(&id), dir.join("sessions").join("42.ghost"));
        notes.set_asking(&id).unwrap();
        notes.set_ghost(&id, "git status").unwrap();
        assert!(notes.asking_path(&id).is_file());
        assert_eq!(
            std::fs::read_to_string(notes.ghost_path(&id)).unwrap(),
            "git status"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}

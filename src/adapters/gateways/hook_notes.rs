//! Small files the shell hooks read at the next prompt, written by the
//! client right after it printed something the hook must know about.
//! The paths are a contract with `shell/kintsu.*`:
//! `<state>/sessions/<id>.asking` says an "asking…" line waits for its
//! answer; `<state>/sessions/<id>.ghost` holds a fix safe enough to be
//! pre-typed on the next prompt; `<state>/sessions/<id>.bubble` holds what
//! kintsu printed above the prompt, as printed, so the panel can take its
//! place and put it back. The hooks remove the bubble when another command
//! runs.

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

    pub fn bubble_path(&self, session: &SessionId) -> PathBuf {
        self.dir.join(format!("{}.bubble", session.as_str()))
    }

    /// What was just printed above the prompt for the current failure.
    pub fn set_bubble(&self, session: &SessionId, text: &str) -> io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        std::fs::write(
            self.bubble_path(session),
            text.trim_end_matches('\n').as_bytes(),
        )
    }

    /// More lines landed under the bubble: a message.
    pub fn append_bubble(&self, session: &SessionId, text: &str) -> io::Result<()> {
        let mut bubble = self.bubble(session)?.unwrap_or_default();
        if !bubble.is_empty() {
            bubble.push('\n');
        }
        bubble.push_str(text.trim_end_matches('\n'));
        self.set_bubble(session, &bubble)
    }

    /// The bubble on screen, when kintsu printed one since the last command.
    pub fn bubble(&self, session: &SessionId) -> io::Result<Option<String>> {
        match std::fs::read_to_string(self.bubble_path(session)) {
            Ok(text) => Ok(Some(text)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
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
        assert_eq!(notes.bubble(&id).unwrap(), None);
        notes
            .set_bubble(&id, "| make exited 2.\n| kintsu fix\n")
            .unwrap();
        notes.append_bubble(&id, "| Try make -j4?\n").unwrap();
        assert_eq!(
            notes.bubble(&id).unwrap().as_deref(),
            Some("| make exited 2.\n| kintsu fix\n| Try make -j4?")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}

//! A file the shell hooks read: "kintsu printed an *asking…* line for this
//! session, the answer may take its place". Written by the client after
//! `kintsu triage` or `kintsu why`, removed by the hook at the next prompt.
//! The path is a contract with `shell/kintsu.*`: `<state>/sessions/<id>.asking`.

use std::io;
use std::path::{Path, PathBuf};

use crate::entities::SessionId;

pub struct AskingMarker {
    dir: PathBuf,
}

impl AskingMarker {
    pub fn new(state_dir: &Path) -> Self {
        Self {
            dir: state_dir.join("sessions"),
        }
    }

    pub fn path(&self, session: &SessionId) -> PathBuf {
        self.dir.join(format!("{}.asking", session.as_str()))
    }

    pub fn set(&self, session: &SessionId) -> io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        std::fs::write(self.path(session), b"")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_marker_is_a_file_named_after_the_session() {
        let dir = std::env::temp_dir().join(format!("kintsu-marker-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let marker = AskingMarker::new(&dir);
        assert_eq!(
            marker.path(&SessionId::new("42")),
            dir.join("sessions").join("42.asking")
        );
        marker.set(&SessionId::new("42")).unwrap();
        assert!(marker.path(&SessionId::new("42")).is_file());
        std::fs::remove_dir_all(dir).unwrap();
    }
}

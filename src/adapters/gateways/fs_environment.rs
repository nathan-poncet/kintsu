//! The machine as the rules see it: system, PATH, a directory listing.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::entities::{DirEntry, Os};
use crate::use_cases::ports::Environment;

/// Shell builtins that never appear on the PATH but are commands all the same.
const BUILTINS: &[&str] = &[
    "alias", "bg", "bind", "break", "builtin", "cd", "command", "continue", "declare", "dirs",
    "disown", "echo", "eval", "exec", "exit", "export", "fg", "hash", "history", "jobs", "kill",
    "let", "local", "popd", "printf", "pushd", "pwd", "read", "return", "set", "shift", "source",
    "test", "time", "times", "trap", "type", "typeset", "ulimit", "umask", "unalias", "unset",
    "wait", "which",
];

pub struct FsEnvironment {
    path: String,
    applications: PathBuf,
}

impl FsEnvironment {
    /// Over the given PATH value.
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            applications: PathBuf::from("/Applications"),
        }
    }

    /// Where applications are installed, for tests: `/Applications` otherwise.
    #[cfg(test)]
    pub fn with_applications(mut self, dir: impl Into<PathBuf>) -> Self {
        self.applications = dir.into();
        self
    }
}

impl Environment for FsEnvironment {
    fn os(&self) -> Option<Os> {
        Some(if cfg!(target_os = "macos") {
            Os::Mac
        } else if cfg!(target_os = "linux") {
            Os::Linux
        } else {
            Os::Other
        })
    }

    fn executables(&self) -> Vec<String> {
        let mut names: BTreeSet<String> = BUILTINS.iter().map(|b| b.to_string()).collect();
        for dir in self.path.split(':').filter(|d| !d.is_empty()) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
            };
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    names.insert(name.to_string());
                }
            }
        }
        names.into_iter().collect()
    }

    fn docker_desktop(&self) -> bool {
        self.applications.join("Docker.app").is_dir()
    }

    fn entries(&self, dir: &str) -> Vec<DirEntry> {
        let Ok(entries) = std::fs::read_dir(Path::new(dir)) else {
            return Vec::new();
        };
        let mut out: Vec<DirEntry> = entries
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_str()?.to_string();
                let meta = entry.metadata().ok()?;
                Some(DirEntry {
                    name,
                    is_dir: meta.is_dir(),
                    is_executable: is_executable(&meta),
                })
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }
}

#[cfg(test)]
mod docker_tests {
    use super::*;

    #[test]
    fn docker_desktop_is_the_app_bundle_in_the_applications_folder() {
        let dir = std::env::temp_dir().join(format!("kintsu-apps-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let env = FsEnvironment::new("/usr/bin").with_applications(&dir);
        assert!(!env.docker_desktop());
        std::fs::create_dir_all(dir.join("Docker.app")).unwrap();
        assert!(env.docker_desktop());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

#[cfg(unix)]
fn is_executable(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.is_dir() || meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(meta: &std::fs::Metadata) -> bool {
    meta.is_dir()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("kintsu-fsenv-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_path_dirs_and_the_builtins_make_the_executables() {
        let dir = scratch("path");
        fs::write(dir.join("mytool"), "").unwrap();
        let env = FsEnvironment::new(format!("{}:/definitely/not/here", dir.display()));
        let execs = env.executables();
        assert!(execs.iter().any(|e| e == "mytool"));
        assert!(execs.iter().any(|e| e == "cd"));
        assert!(env.os().is_some());
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn directory_entries_know_who_is_a_dir_and_who_runs() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("entries");
        fs::create_dir(dir.join("apps")).unwrap();
        fs::write(dir.join("notes.md"), "").unwrap();
        fs::write(dir.join("deploy.sh"), "").unwrap();
        fs::set_permissions(dir.join("deploy.sh"), fs::Permissions::from_mode(0o755)).unwrap();
        let entries = FsEnvironment::new("").entries(dir.to_str().unwrap());
        let find = |n: &str| entries.iter().find(|e| e.name == n).unwrap();
        assert!(find("apps").is_dir);
        assert!(find("deploy.sh").is_executable && !find("deploy.sh").is_dir);
        assert!(!find("notes.md").is_executable);
        assert!(
            FsEnvironment::new("")
                .entries("/definitely/not/here")
                .is_empty()
        );
        fs::remove_dir_all(dir).unwrap();
    }
}

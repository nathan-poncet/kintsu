//! The OS keychain, written to: `security` on macOS, `secret-tool` on
//! Linux, found on the shell's PATH. Reading is `EnvSecrets`' job.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::entities::SecretKey;
use crate::use_cases::ports::{SecretStore, SecretStoreError};

/// The keychain service every kintsu entry is filed under.
pub const SERVICE: &str = "kintsu";

#[cfg(target_os = "macos")]
pub const PROGRAM: &str = "security";
#[cfg(not(target_os = "macos"))]
pub const PROGRAM: &str = "secret-tool";

pub struct OsKeychain {
    path_var: String,
}

impl OsKeychain {
    /// Looks the program up on this PATH value.
    pub fn new(path_var: impl Into<String>) -> Self {
        Self {
            path_var: path_var.into(),
        }
    }

    fn program(&self) -> Option<PathBuf> {
        self.path_var
            .split(':')
            .filter(|dir| !dir.is_empty())
            .map(|dir| PathBuf::from(dir).join(PROGRAM))
            .find(|candidate| candidate.is_file())
    }
}

impl SecretStore for OsKeychain {
    fn store(&self, account: &str, key: &SecretKey) -> Result<(), SecretStoreError> {
        let program = self.program().ok_or_else(|| {
            SecretStoreError::Unavailable(format!("`{PROGRAM}` is not on the PATH"))
        })?;
        let mut command = Command::new(program);
        command.stdout(Stdio::null()).stderr(Stdio::piped());
        // macOS takes the password as an argument, Linux reads it on stdin.
        if cfg!(target_os = "macos") {
            command
                .args([
                    "add-generic-password",
                    "-U",
                    "-s",
                    SERVICE,
                    "-a",
                    account,
                    "-w",
                    key.expose(),
                ])
                .stdin(Stdio::null());
        } else {
            command
                .args([
                    "store",
                    &format!("--label={SERVICE}"),
                    "service",
                    SERVICE,
                    "account",
                    account,
                ])
                .stdin(Stdio::piped());
        }
        let mut child = command
            .spawn()
            .map_err(|e| SecretStoreError::Unavailable(format!("cannot run `{PROGRAM}`: {e}")))?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(key.expose().as_bytes());
        }
        let out = child.wait_with_output().map_err(|e| {
            SecretStoreError::Unavailable(format!("`{PROGRAM}` did not finish: {e}"))
        })?;
        if out.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(SecretStoreError::Refused(if stderr.is_empty() {
            format!("`{PROGRAM}` exited {}", out.status.code().unwrap_or(-1))
        } else {
            stderr
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("kintsu-keychain-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn fake_program(dir: &std::path::Path, body: &str) {
        let path = dir.join(PROGRAM);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
    }

    #[test]
    fn the_key_reaches_the_program_the_way_the_platform_wants_it() {
        let dir = scratch("store");
        let log = dir.join("log");
        fake_program(
            &dir,
            &format!(
                "printf '%s\\n' \"$@\" > {log}; cat >> {log}",
                log = log.display()
            ),
        );
        let store = OsKeychain::new(dir.display().to_string());
        store
            .store("haiku", &SecretKey::new("sk-test").unwrap())
            .unwrap();
        let logged = std::fs::read_to_string(&log).unwrap();
        if cfg!(target_os = "macos") {
            assert_eq!(
                logged,
                "add-generic-password\n-U\n-s\nkintsu\n-a\nhaiku\n-w\nsk-test\n"
            );
        } else {
            assert_eq!(
                logged,
                "store\n--label=kintsu\nservice\nkintsu\naccount\nhaiku\nsk-test"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_refusal_carries_the_programs_words_and_a_missing_program_is_said() {
        let dir = scratch("refuse");
        fake_program(&dir, "echo 'User interaction is not allowed.' >&2; exit 36");
        let store = OsKeychain::new(dir.display().to_string());
        assert_eq!(
            store.store("haiku", &SecretKey::new("k").unwrap()),
            Err(SecretStoreError::Refused(
                "User interaction is not allowed.".into()
            ))
        );
        let nowhere = OsKeychain::new(dir.join("empty").display().to_string());
        assert!(matches!(
            nowhere.store("haiku", &SecretKey::new("k").unwrap()),
            Err(SecretStoreError::Unavailable(_))
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

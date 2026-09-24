//! Keys from the environment, a command, or the OS keychain.

use std::process::{Command, Stdio};

use crate::entities::KeySource;
use crate::use_cases::ports::Secrets;

pub struct EnvSecrets;

impl Secrets for EnvSecrets {
    fn lookup(&self, source: &KeySource) -> Option<String> {
        match source {
            KeySource::None => None,
            KeySource::Literal(key) => Some(key.clone()),
            KeySource::Env(var) => std::env::var(var)
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty()),
            KeySource::Command(command) => output_of("sh", &["-c", command]),
            KeySource::Keychain(account) => keychain(account),
        }
    }
}

fn output_of(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let key = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!key.is_empty()).then_some(key)
}

#[cfg(target_os = "macos")]
fn keychain(account: &str) -> Option<String> {
    output_of(
        "security",
        &["find-generic-password", "-s", "kintsu", "-a", account, "-w"],
    )
}

#[cfg(not(target_os = "macos"))]
fn keychain(account: &str) -> Option<String> {
    output_of(
        "secret-tool",
        &["lookup", "service", "kintsu", "account", account],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_commands_and_missing_variables_resolve_as_expected() {
        assert_eq!(
            EnvSecrets
                .lookup(&KeySource::Literal("k".into()))
                .as_deref(),
            Some("k")
        );
        assert_eq!(
            EnvSecrets
                .lookup(&KeySource::Command("printf '  from-cmd \\n'".into()))
                .as_deref(),
            Some("from-cmd")
        );
        assert_eq!(
            EnvSecrets.lookup(&KeySource::Command("exit 3".into())),
            None
        );
        assert_eq!(
            EnvSecrets.lookup(&KeySource::Env("KINTSU_TEST_SURELY_UNSET_VAR".into())),
            None
        );
        assert_eq!(EnvSecrets.lookup(&KeySource::None), None);
    }
}

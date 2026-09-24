//! Whether a command line deserves a red line before anyone presses Enter.

use std::fmt;

/// The classification of a command's potential for harm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Danger {
    /// Nothing a warning is needed for.
    None,
    /// Runs with elevated privileges; the user should know why.
    NeedsPrivilege,
    /// Deletes, overwrites, rewrites history or pipes the internet into a shell.
    Destructive(&'static str),
}

impl fmt::Display for Danger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Danger::None => f.write_str("not destructive"),
            Danger::NeedsPrivilege => f.write_str("runs as root"),
            Danger::Destructive(why) => write!(f, "⚠ {why}"),
        }
    }
}

/// The patterns a command line is checked against, in order. Word
/// boundaries matter: `format` in a sentence is not `format`ting a disk.
const DESTRUCTIVE: &[(&str, &str)] = &[
    ("rm -rf", "deletes recursively"),
    ("rm -fr", "deletes recursively"),
    ("rm -r", "deletes recursively"),
    ("git push --force", "rewrites remote history"),
    ("git push -f", "rewrites remote history"),
    ("git reset --hard", "discards local changes"),
    ("git clean -fd", "deletes untracked files"),
    ("git checkout -- .", "discards local changes"),
    ("drop table", "drops a table"),
    ("drop database", "drops a database"),
    ("truncate table", "empties a table"),
    ("chmod 777", "opens permissions to everyone"),
    ("chmod -r 777", "opens permissions to everyone"),
    ("mkfs", "formats a filesystem"),
    ("dd if=", "writes raw blocks"),
    ("> /dev/sd", "writes raw blocks"),
    ("kill -9", "kills without cleanup"),
    ("killall", "kills every matching process"),
    ("pkill", "kills every matching process"),
    ("shutdown", "shuts the machine down"),
    ("reboot", "reboots the machine"),
    (":(){ :|:& };:", "fork bomb"),
];

/// Classifies a command line. `kill <pid>` is destructive too: it ends a
/// process the user did not necessarily start.
pub fn classify_danger(command: &str) -> Danger {
    let lower = command.to_lowercase();
    let squeezed: String = lower.split_whitespace().collect::<Vec<_>>().join(" ");
    if squeezed.contains("curl") && squeezed.contains("| sh") || squeezed.contains("| bash") {
        return Danger::Destructive("runs a downloaded script");
    }
    for (pattern, why) in DESTRUCTIVE {
        if squeezed.contains(pattern) {
            return Danger::Destructive(why);
        }
    }
    if squeezed.starts_with("kill ")
        || squeezed.contains(" && kill ")
        || squeezed.contains("; kill ")
    {
        return Danger::Destructive("ends a process");
    }
    if squeezed.starts_with("sudo ") || squeezed.starts_with("doas ") {
        return Danger::NeedsPrivilege;
    }
    Danger::None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deleting_forcing_and_formatting_are_destructive() {
        for cmd in [
            "rm -rf build",
            "git push --force origin main",
            "git reset --hard",
            "sudo mkfs.ext4 /dev/sda1",
            "kill 4821",
            "curl -fsSL https://x | sh",
            "DROP TABLE users;",
        ] {
            assert!(
                matches!(classify_danger(cmd), Danger::Destructive(_)),
                "{cmd}"
            );
        }
    }

    #[test]
    fn sudo_alone_needs_privilege_not_a_red_line() {
        assert_eq!(
            classify_danger("sudo apt install jq"),
            Danger::NeedsPrivilege
        );
    }

    #[test]
    fn ordinary_commands_are_not_flagged() {
        for cmd in [
            "git status",
            "nvm use 22 && npm run build",
            "ls -la",
            "cargo test",
            "echo format the report",
        ] {
            assert_eq!(classify_danger(cmd), Danger::None, "{cmd}");
        }
    }

    #[test]
    fn extra_spaces_do_not_hide_a_pattern() {
        assert!(matches!(
            classify_danger("rm   -rf   /tmp/x"),
            Danger::Destructive(_)
        ));
    }
}

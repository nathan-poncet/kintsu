//! Instant fixes that need no model: a rule looks at the outcome and at a
//! few facts about the machine, and proposes one command or nothing.

use crate::entities::{CommandLine, CommandOutcome, Confidence, Fix, FixSource, distance::closest};

/// Which family of operating system the shell runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    /// macOS.
    Mac,
    /// Any Linux.
    Linux,
    /// Anything else.
    Other,
}

/// One entry of the working directory, as far as rules care.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// The file or directory name.
    pub name: String,
    /// Whether it is a directory.
    pub is_dir: bool,
    /// Whether the user may execute it.
    pub is_executable: bool,
}

/// What the rules may know about the machine, gathered by a port.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    /// The operating system family.
    pub os: Option<Os>,
    /// Every program name on the PATH, plus shell builtins and aliases.
    pub executables: Vec<String>,
    /// The entries of the working directory.
    pub cwd_entries: Vec<DirEntry>,
}

/// Git's porcelain subcommands, for `git stauts`.
const GIT_SUBCOMMANDS: &[&str] = &[
    "add",
    "am",
    "archive",
    "bisect",
    "blame",
    "branch",
    "bundle",
    "checkout",
    "cherry-pick",
    "clean",
    "clone",
    "commit",
    "config",
    "describe",
    "diff",
    "fetch",
    "format-patch",
    "gc",
    "grep",
    "init",
    "log",
    "merge",
    "mv",
    "notes",
    "pull",
    "push",
    "range-diff",
    "rebase",
    "reflog",
    "remote",
    "reset",
    "restore",
    "revert",
    "rm",
    "shortlog",
    "show",
    "stash",
    "status",
    "submodule",
    "switch",
    "tag",
    "worktree",
];

/// Cargo's common subcommands, for `cargo biuld`.
const CARGO_SUBCOMMANDS: &[&str] = &[
    "add",
    "bench",
    "build",
    "check",
    "clean",
    "clippy",
    "doc",
    "fetch",
    "fix",
    "fmt",
    "init",
    "install",
    "metadata",
    "new",
    "publish",
    "remove",
    "run",
    "search",
    "test",
    "tree",
    "uninstall",
    "update",
];

/// The rules, in the order they are tried. The first fix wins.
pub fn suggest_fix(outcome: &CommandOutcome, facts: &Facts) -> Option<Fix> {
    let rules: [fn(&CommandOutcome, &Facts) -> Option<Fix>; 5] = [
        command_typo,
        subcommand_typo,
        missing_dot_slash,
        cd_into_file,
        package_manager,
    ];
    rules.iter().find_map(|rule| rule(outcome, facts))
}

/// `gti status`: the program is not on the PATH, one program is a typo away.
fn command_typo(outcome: &CommandOutcome, facts: &Facts) -> Option<Fix> {
    if !outcome.status().is_command_not_found() {
        return None;
    }
    let program = outcome.command().program();
    if program.len() < 2 || facts.executables.iter().any(|e| e == program) {
        return None;
    }
    let max = if program.len() <= 4 { 1 } else { 2 };
    let found = closest(program, facts.executables.iter().map(String::as_str), max)?;
    Some(Fix::new(
        outcome.command().with_program(found),
        Confidence::new(if max == 1 { 0.9 } else { 0.85 }),
        FixSource::Rule("command typo"),
        format!("`{program}` is not on your PATH; `{found}` is."),
    ))
}

/// `git stauts`, `cargo biuld`: the program is fine, the subcommand is a typo.
fn subcommand_typo(outcome: &CommandOutcome, _facts: &Facts) -> Option<Fix> {
    if outcome.status().is_success() || outcome.status().is_command_not_found() {
        return None;
    }
    let words = outcome.command().words();
    let (program, sub) = (words.first()?, words.get(1)?);
    let known: &[&str] = match *program {
        "git" => GIT_SUBCOMMANDS,
        "cargo" => CARGO_SUBCOMMANDS,
        _ => return None,
    };
    if sub.starts_with('-') || known.contains(sub) {
        return None;
    }
    let found = closest(sub, known.iter().copied(), 2)?;
    let mut fixed: Vec<&str> = words.clone();
    fixed[1] = found;
    Some(Fix::new(
        CommandLine::new(fixed.join(" ")).ok()?,
        Confidence::new(0.85),
        FixSource::Rule("subcommand typo"),
        format!("`{program} {sub}` is not a {program} command; `{program} {found}` is."),
    ))
}

/// `deploy.sh` when `./deploy.sh` is right here and executable.
fn missing_dot_slash(outcome: &CommandOutcome, facts: &Facts) -> Option<Fix> {
    if !outcome.status().is_command_not_found() {
        return None;
    }
    let program = outcome.command().program();
    if program.contains('/') {
        return None;
    }
    let here = facts
        .cwd_entries
        .iter()
        .find(|e| e.name == program && !e.is_dir && e.is_executable)?;
    Some(Fix::new(
        outcome.command().with_program(&format!("./{}", here.name)),
        Confidence::new(0.9),
        FixSource::Rule("missing ./"),
        format!("`{program}` is in this directory, not on your PATH."),
    ))
}

/// `cd api` when `api` is a file, or is not here but `apps/api` is: only the
/// first case is decidable from the working directory alone.
fn cd_into_file(outcome: &CommandOutcome, facts: &Facts) -> Option<Fix> {
    if outcome.status().is_success() || outcome.command().program() != "cd" {
        return None;
    }
    let target = outcome.command().arguments().first().copied()?;
    if target.contains('/') || target.starts_with('-') {
        return None;
    }
    let dirs: Vec<&str> = facts
        .cwd_entries
        .iter()
        .filter(|e| e.is_dir)
        .map(|e| e.name.as_str())
        .collect();
    if dirs.contains(&target) {
        return None;
    }
    let found = closest(target, dirs.iter().copied(), 2)?;
    Some(Fix::new(
        CommandLine::new(format!("cd {found}")).ok()?,
        Confidence::new(0.8),
        FixSource::Rule("directory typo"),
        format!("There is no `{target}` here; `{found}` is a directory."),
    ))
}

/// `apt install jq` on a Mac: the package manager of another system.
fn package_manager(outcome: &CommandOutcome, facts: &Facts) -> Option<Fix> {
    if !outcome.status().is_command_not_found() || facts.os != Some(Os::Mac) {
        return None;
    }
    let words = outcome.command().words();
    let program = *words.first()?;
    if !matches!(program, "apt" | "apt-get" | "dnf" | "yum" | "pacman") {
        return None;
    }
    if !facts.executables.iter().any(|e| e == "brew") {
        return None;
    }
    let rest: Vec<&str> = words
        .iter()
        .skip(1)
        .filter(|w| **w != "-y")
        .copied()
        .collect();
    let verb = rest.first().copied().unwrap_or("install");
    let verb = match (program, verb) {
        ("pacman", "-S") => "install",
        (_, "remove") | (_, "purge") | ("pacman", "-R") => "uninstall",
        (_, "update") | (_, "upgrade") => "upgrade",
        (_, v) => v,
    };
    let packages: Vec<&str> = rest.iter().skip(1).copied().collect();
    let command = if packages.is_empty() {
        format!("brew {verb}")
    } else {
        format!("brew {verb} {}", packages.join(" "))
    };
    Some(Fix::new(
        CommandLine::new(command).ok()?,
        Confidence::new(0.8),
        FixSource::Rule("package manager"),
        format!("`{program}` is not on macOS; Homebrew is."),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{Danger, ExitStatus};

    fn outcome(text: &str, code: i32) -> CommandOutcome {
        CommandOutcome::new(CommandLine::new(text).unwrap(), ExitStatus::new(code))
    }
    fn facts(execs: &[&str]) -> Facts {
        Facts {
            os: Some(Os::Linux),
            executables: execs.iter().map(|s| s.to_string()).collect(),
            cwd_entries: vec![],
        }
    }
    fn entry(name: &str, is_dir: bool, is_executable: bool) -> DirEntry {
        DirEntry {
            name: name.into(),
            is_dir,
            is_executable,
        }
    }

    #[test]
    fn a_program_one_typo_away_is_corrected_keeping_the_arguments() {
        let fix = suggest_fix(
            &outcome("gti status --short", 127),
            &facts(&["git", "go", "grep"]),
        )
        .unwrap();
        assert_eq!(fix.command().as_str(), "git status --short");
        assert!(fix.confidence().is_high());
        assert_eq!(fix.source(), &FixSource::Rule("command typo"));
        assert_eq!(fix.danger(), &Danger::None);
    }

    #[test]
    fn short_programs_only_tolerate_one_edit_and_ties_are_refused() {
        assert!(
            suggest_fix(&outcome("gxx", 127), &facts(&["git"])).is_none(),
            "two edits on a 3-letter word"
        );
        assert!(
            suggest_fix(&outcome("gut", 127), &facts(&["git", "gat"])).is_none(),
            "a tie"
        );
    }

    #[test]
    fn a_command_that_exists_but_failed_is_not_a_typo() {
        assert!(suggest_fix(&outcome("git status", 1), &facts(&["git"])).is_none());
        assert!(
            suggest_fix(&outcome("git status", 127), &facts(&["git"])).is_none(),
            "on the PATH, so 127 is something else"
        );
    }

    #[test]
    fn a_git_or_cargo_subcommand_typo_is_corrected() {
        let fix = suggest_fix(&outcome("git stauts", 1), &facts(&["git"])).unwrap();
        assert_eq!(fix.command().as_str(), "git status");
        let fix = suggest_fix(&outcome("cargo biuld --release", 101), &facts(&["cargo"])).unwrap();
        assert_eq!(fix.command().as_str(), "cargo build --release");
        assert!(
            suggest_fix(&outcome("git status", 1), &facts(&["git"])).is_none(),
            "a real subcommand that failed"
        );
        assert!(
            suggest_fix(&outcome("git --version", 1), &facts(&["git"])).is_none(),
            "a flag is not a subcommand"
        );
    }

    #[test]
    fn a_local_executable_gets_its_dot_slash() {
        let mut f = facts(&["git"]);
        f.cwd_entries = vec![
            entry("deploy.sh", false, true),
            entry("notes.md", false, false),
        ];
        let fix = suggest_fix(&outcome("deploy.sh --prod", 127), &f).unwrap();
        assert_eq!(fix.command().as_str(), "./deploy.sh --prod");
        assert!(
            suggest_fix(&outcome("notes.md", 127), &f).is_none(),
            "not executable"
        );
    }

    #[test]
    fn cd_into_a_directory_that_is_almost_there_is_corrected() {
        let mut f = facts(&["git"]);
        f.cwd_entries = vec![entry("apps", true, true), entry("api.txt", false, false)];
        let fix = suggest_fix(&outcome("cd aps", 1), &f).unwrap();
        assert_eq!(fix.command().as_str(), "cd apps");
        assert!(
            suggest_fix(&outcome("cd apps", 1), &f).is_none(),
            "the directory exists: something else failed"
        );
        assert!(
            suggest_fix(&outcome("cd ../x", 1), &f).is_none(),
            "paths are left alone"
        );
    }

    #[test]
    fn apt_on_a_mac_becomes_brew_when_brew_is_there() {
        let mut f = facts(&["brew", "git"]);
        f.os = Some(Os::Mac);
        let fix = suggest_fix(&outcome("apt install jq", 127), &f).unwrap();
        assert_eq!(fix.command().as_str(), "brew install jq");
        let fix = suggest_fix(&outcome("sudo apt-get -y remove jq", 127), &f);
        assert!(
            fix.is_none(),
            "sudo is the program here; the rule does not look past it"
        );
        f.os = Some(Os::Linux);
        assert!(
            suggest_fix(&outcome("apt install jq", 127), &f).is_none(),
            "on Linux apt may simply be missing"
        );
    }

    #[test]
    fn the_first_matching_rule_wins() {
        let mut f = facts(&["git"]);
        f.cwd_entries = vec![entry("gti", false, true)];
        let fix = suggest_fix(&outcome("gti status", 127), &f).unwrap();
        assert_eq!(
            fix.command().as_str(),
            "git status",
            "typo rule comes before ./ rule"
        );
    }
}

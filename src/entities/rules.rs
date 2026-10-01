//! Instant fixes that need no model: a rule looks at the outcome and at a
//! few facts about the machine, and proposes one command or nothing. The
//! command's output is not among the facts: these rules run on the quiet
//! path, before anything is read from the terminal. Several follow rules
//! of thefuck (MIT), redone for what the hook knows at that moment.

use crate::entities::subcommands::{GIT_LONG_OPTIONS, programs_with_subcommands, subcommands_of};
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
    /// Whether Docker Desktop is installed; asked only when the output
    /// speaks of Docker.
    pub docker_desktop: bool,
}

impl Facts {
    pub(crate) fn has_program(&self, name: &str) -> bool {
        self.executables.iter().any(|e| e == name)
    }

    pub(crate) fn entry(&self, name: &str) -> Option<&DirEntry> {
        self.cwd_entries.iter().find(|e| e.name == name)
    }

    pub(crate) fn is_dir_here(&self, name: &str) -> bool {
        let name = name.strip_suffix('/').unwrap_or(name);
        self.entry(name).is_some_and(|e| e.is_dir)
    }

    pub(crate) fn is_file_here(&self, name: &str) -> bool {
        self.entry(name).is_some_and(|e| !e.is_dir)
    }
}

type Rule = fn(&CommandOutcome, &Facts) -> Option<Fix>;

/// The rules, in the order they are tried; the first fix wins. The ones
/// that read the shape of the line come before the ones that guess a
/// typo, so `git-log` becomes `git log` and not `git-lfs`.
pub fn suggest_fix(outcome: &CommandOutcome, facts: &Facts) -> Option<Fix> {
    let rules: &[Rule] = &[
        pasted_prompt,
        no_break_space,
        trailing_cedilla,
        cd_without_space,
        hyphen_before_subcommand,
        missing_space_before_subcommand,
        man_without_space,
        gradle_wrapper,
        venv_here,
        command_typo,
        missing_dot_slash,
        script_without_interpreter,
        repeated_program,
        subcommand_typo,
        git_option_with_one_dash,
        source_without_extension,
        not_executable_yet,
        cat_on_a_directory,
        rm_on_a_directory,
        mkdir_without_parent,
        cd_into_file,
        package_manager,
    ];
    rules.iter().find_map(|rule| rule(outcome, facts))
}

fn fix(command: String, confidence: f32, rule: &str, rationale: String) -> Option<Fix> {
    Some(Fix::new(
        CommandLine::new(command).ok()?,
        Confidence::new(confidence),
        FixSource::Rule(rule.into()),
        rationale,
    ))
}

/// The first argument that is not a flag, for `cat -n dir`.
fn first_operand<'a>(words: &[&'a str]) -> Option<&'a str> {
    words.iter().skip(1).find(|w| !w.starts_with('-')).copied()
}

/// `$ git status`, pasted from a documentation with its prompt.
fn pasted_prompt(outcome: &CommandOutcome, _facts: &Facts) -> Option<Fix> {
    if !outcome.status().is_command_not_found() || outcome.command().program() != "$" {
        return None;
    }
    let rest = outcome.command().arguments();
    if rest.is_empty() {
        return None;
    }
    fix(
        rest.join(" "),
        0.95,
        "pasted prompt",
        "The `$` is the prompt of the page this was copied from, not part of the command.".into(),
    )
}

/// `ls\u{a0}-la`: Alt-Space typed a no-break space where a space was meant.
fn no_break_space(outcome: &CommandOutcome, _facts: &Facts) -> Option<Fix> {
    let text = outcome.command().as_str();
    if outcome.status().is_success() || !text.contains('\u{a0}') {
        return None;
    }
    fix(
        text.replace('\u{a0}', " "),
        0.9,
        "no-break space",
        "There is a no-break space in the line, the kind Alt-Space types; the shell does not split on it.".into(),
    )
}

/// `lsç`, `git statusç`: the key next to Enter on French keyboards.
fn trailing_cedilla(outcome: &CommandOutcome, _facts: &Facts) -> Option<Fix> {
    let text = outcome.command().as_str().trim_end();
    if outcome.status().is_success() || !text.ends_with('ç') {
        return None;
    }
    fix(
        text.trim_end_matches('ç').to_string(),
        0.9,
        "trailing cedilla",
        "A `ç` slipped in at the end of the line: its key sits next to Enter.".into(),
    )
}

/// `cd..` for `cd ..`.
fn cd_without_space(outcome: &CommandOutcome, _facts: &Facts) -> Option<Fix> {
    if !outcome.status().is_command_not_found() || outcome.command().program() != "cd.." {
        return None;
    }
    fix(
        outcome.command().with_program("cd ..").as_str().to_string(),
        0.95,
        "cd without a space",
        "`cd..` is not a program; `cd ..` goes up one directory.".into(),
    )
}

/// `git-log`, `apt-install`: a hyphen where the space between a program
/// and its subcommand should be.
fn hyphen_before_subcommand(outcome: &CommandOutcome, facts: &Facts) -> Option<Fix> {
    if !outcome.status().is_command_not_found() {
        return None;
    }
    let program = outcome.command().program();
    if program.starts_with('-') {
        return None;
    }
    let (head, tail) = program
        .match_indices('-')
        .map(|(i, _)| (&program[..i], &program[i + 1..]))
        .find(|(head, tail)| {
            facts.has_program(head) && subcommands_of(head).is_some_and(|s| s.contains(tail))
        })?;
    fix(
        outcome
            .command()
            .with_program(&format!("{head} {tail}"))
            .as_str()
            .to_string(),
        0.9,
        "hyphen before subcommand",
        format!("`{program}` is not a program; `{head} {tail}` is the command."),
    )
}

/// `gitpush`, `npminstall`: the space between a program and its
/// subcommand is missing.
fn missing_space_before_subcommand(outcome: &CommandOutcome, facts: &Facts) -> Option<Fix> {
    if !outcome.status().is_command_not_found() {
        return None;
    }
    let program = outcome.command().program();
    let (head, tail) = programs_with_subcommands()
        .filter(|head| program.len() > head.len() && program.starts_with(head))
        .map(|head| (head, &program[head.len()..]))
        .find(|(head, tail)| {
            facts.has_program(head) && subcommands_of(head).is_some_and(|s| s.contains(tail))
        })?;
    fix(
        outcome
            .command()
            .with_program(&format!("{head} {tail}"))
            .as_str()
            .to_string(),
        0.9,
        "missing space",
        format!("`{program}` is `{head} {tail}` without its space."),
    )
}

/// `mandiff` for `man diff`.
fn man_without_space(outcome: &CommandOutcome, facts: &Facts) -> Option<Fix> {
    if !outcome.status().is_command_not_found() {
        return None;
    }
    let program = outcome.command().program();
    let page = program.strip_prefix("man")?;
    if page.len() < 2 || !facts.has_program(page) || !facts.has_program("man") {
        return None;
    }
    fix(
        outcome
            .command()
            .with_program(&format!("man {page}"))
            .as_str()
            .to_string(),
        0.85,
        "man without a space",
        format!("`{program}` is not a program; `man {page}` opens the manual of `{page}`."),
    )
}

/// `gradle build` when there is no gradle, but the project's `./gradlew`.
fn gradle_wrapper(outcome: &CommandOutcome, facts: &Facts) -> Option<Fix> {
    if !outcome.status().is_command_not_found() || outcome.command().program() != "gradle" {
        return None;
    }
    let wrapper = facts.entry("gradlew")?;
    if wrapper.is_dir || !wrapper.is_executable {
        return None;
    }
    fix(
        outcome
            .command()
            .with_program("./gradlew")
            .as_str()
            .to_string(),
        0.9,
        "gradle wrapper",
        "There is no `gradle` on your PATH; this project ships its wrapper.".into(),
    )
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
        FixSource::Rule("missing ./".into()),
        format!("`{program}` is in this directory, not on your PATH."),
    ))
}

/// Interpreters by extension, the first one on the PATH wins.
const INTERPRETERS: &[(&str, &[&str])] = &[
    ("py", &["python3", "python"]),
    ("js", &["node"]),
    ("rb", &["ruby"]),
    ("pl", &["perl"]),
    ("sh", &["bash", "sh"]),
];

/// `script.py` when the file is here but not executable: the interpreter
/// runs it.
fn script_without_interpreter(outcome: &CommandOutcome, facts: &Facts) -> Option<Fix> {
    if !outcome.status().is_command_not_found() {
        return None;
    }
    let program = outcome.command().program();
    let (_, extension) = program.rsplit_once('.')?;
    if program.contains('/') || !facts.is_file_here(program) {
        return None;
    }
    let (_, candidates) = INTERPRETERS.iter().find(|(ext, _)| *ext == extension)?;
    let interpreter = candidates.iter().find(|i| facts.has_program(i))?;
    fix(
        format!("{interpreter} {}", outcome.command().as_str()),
        0.9,
        "script without interpreter",
        format!("`{program}` is here but not executable; `{interpreter}` can run it."),
    )
}

/// Python's usual tools, the ones a project installs in its own `.venv`.
const VENV_TOOLS: &[&str] = &[
    "alembic",
    "black",
    "celery",
    "coverage",
    "django-admin",
    "flake8",
    "flask",
    "gunicorn",
    "ipython",
    "isort",
    "jupyter",
    "mkdocs",
    "mypy",
    "pip",
    "pip3",
    "pre-commit",
    "pylint",
    "pytest",
    "python",
    "python3",
    "ruff",
    "sphinx-build",
    "tox",
    "uvicorn",
];

/// `pytest` not found while `.venv/` is here: the project's own tool,
/// not activated. Before the typo guess, which would offer `pip3` or
/// `pipx` for `pip`.
fn venv_here(outcome: &CommandOutcome, facts: &Facts) -> Option<Fix> {
    if !outcome.status().is_command_not_found() {
        return None;
    }
    let program = outcome.command().program();
    if !VENV_TOOLS.contains(&program) || !facts.is_dir_here(".venv") {
        return None;
    }
    fix(
        outcome
            .command()
            .with_program(&format!(".venv/bin/{program}"))
            .as_str()
            .to_string(),
        0.7,
        "venv here",
        format!(
            "`{program}` is not on your PATH, but this project has a `.venv`: its `{program}` is in there."
        ),
    )
}

/// `gti status`: the program is not on the PATH, one program is a typo away.
fn command_typo(outcome: &CommandOutcome, facts: &Facts) -> Option<Fix> {
    if !outcome.status().is_command_not_found() {
        return None;
    }
    let program = outcome.command().program();
    if program.len() < 2 || facts.has_program(program) {
        return None;
    }
    let max = if program.len() <= 4 { 1 } else { 2 };
    let found = closest(program, facts.executables.iter().map(String::as_str), max)?;
    Some(Fix::new(
        outcome.command().with_program(found),
        Confidence::new(if max == 1 { 0.9 } else { 0.85 }),
        FixSource::Rule("command typo".into()),
        format!("`{program}` is not on your PATH; `{found}` is."),
    ))
}

/// `git git push`: the program typed twice.
fn repeated_program(outcome: &CommandOutcome, facts: &Facts) -> Option<Fix> {
    if outcome.status().is_success() {
        return None;
    }
    let words = outcome.command().words();
    let (first, second) = (words.first()?, words.get(1)?);
    if first != second || !(facts.has_program(first) || subcommands_of(first).is_some()) {
        return None;
    }
    let mut fixed = words.clone();
    fixed.remove(1);
    fix(
        fixed.join(" "),
        0.9,
        "repeated program",
        format!("`{first}` is there twice."),
    )
}

/// `git stauts`, `cargo biuld`: the program is fine, the subcommand is a typo.
fn subcommand_typo(outcome: &CommandOutcome, _facts: &Facts) -> Option<Fix> {
    if outcome.status().is_success() || outcome.status().is_command_not_found() {
        return None;
    }
    let words = outcome.command().words();
    let (program, sub) = (words.first()?, words.get(1)?);
    let known = subcommands_of(program)?;
    if sub.starts_with('-') || sub.contains('/') || known.contains(sub) {
        return None;
    }
    let max = if sub.len() <= 3 { 1 } else { 2 };
    let found = closest(sub, known.iter().copied(), max)?;
    let mut fixed: Vec<&str> = words.clone();
    fixed[1] = found;
    Some(Fix::new(
        CommandLine::new(fixed.join(" ")).ok()?,
        Confidence::new(0.85),
        FixSource::Rule("subcommand typo".into()),
        format!("`{program} {sub}` is not a {program} command; `{program} {found}` is."),
    ))
}

/// `git commit -amend`: a long option with one dash, which git reads as
/// short options.
fn git_option_with_one_dash(outcome: &CommandOutcome, _facts: &Facts) -> Option<Fix> {
    if outcome.status().is_success() || outcome.command().program() != "git" {
        return None;
    }
    let words = outcome.command().words();
    let (index, name) = words.iter().enumerate().skip(2).find_map(|(i, w)| {
        let name = w.strip_prefix('-')?;
        (!name.starts_with('-') && name.len() >= 3 && GIT_LONG_OPTIONS.contains(&name))
            .then_some((i, name))
    })?;
    let mut fixed = words.clone();
    let long = format!("--{name}");
    fixed[index] = &long;
    fix(
        fixed.join(" "),
        0.75,
        "one dash",
        format!("`-{name}` reads as a string of short options; `--{name}` is the option."),
    )
}

/// `python app` when `app.py` is here, `go run main` when `main.go` is.
fn source_without_extension(outcome: &CommandOutcome, facts: &Facts) -> Option<Fix> {
    if outcome.status().is_success() || outcome.status().is_command_not_found() {
        return None;
    }
    let words = outcome.command().words();
    let (index, extension) = match (words.first()?, words.get(1)) {
        (&"python" | &"python3", Some(_)) => (1, "py"),
        (&"ruby", Some(_)) => (1, "rb"),
        (&"go", Some(&"run")) => (2, "go"),
        _ => return None,
    };
    let target = words.get(index)?;
    if target.starts_with('-') || target.contains('/') || target.contains('.') {
        return None;
    }
    let with_extension = format!("{target}.{extension}");
    if facts.entry(target).is_some() || !facts.is_file_here(&with_extension) {
        return None;
    }
    let mut fixed = words.clone();
    fixed[index] = &with_extension;
    fix(
        fixed.join(" "),
        0.85,
        "missing extension",
        format!("There is no `{target}` here; `{with_extension}` is the file."),
    )
}

/// `./deploy.sh` refused with 126: the file is here without its execute bit.
fn not_executable_yet(outcome: &CommandOutcome, facts: &Facts) -> Option<Fix> {
    if !outcome.status().is_not_executable() {
        return None;
    }
    let program = outcome.command().program();
    let name = program.strip_prefix("./")?;
    if name.contains('/') {
        return None;
    }
    let here = facts.entry(name)?;
    if here.is_dir || here.is_executable {
        return None;
    }
    fix(
        format!("chmod +x {name} && {}", outcome.command().as_str()),
        0.85,
        "not executable",
        format!("`{name}` has no execute bit yet."),
    )
}

/// `cat src` when `src` is a directory: `ls` is what shows one.
fn cat_on_a_directory(outcome: &CommandOutcome, facts: &Facts) -> Option<Fix> {
    if outcome.status().is_success() || outcome.command().program() != "cat" {
        return None;
    }
    let words = outcome.command().words();
    let target = first_operand(&words)?;
    if target.contains('/') && !target.ends_with('/') || !facts.is_dir_here(target) {
        return None;
    }
    fix(
        outcome.command().with_program("ls").as_str().to_string(),
        0.85,
        "cat on a directory",
        format!("`{target}` is a directory; `ls` lists one."),
    )
}

/// `rm build` when `build` is a directory: `-r` is needed, and the fix
/// carries the red line any recursive removal gets.
fn rm_on_a_directory(outcome: &CommandOutcome, facts: &Facts) -> Option<Fix> {
    if outcome.status().is_success() || outcome.command().program() != "rm" {
        return None;
    }
    let words = outcome.command().words();
    let recursive = words
        .iter()
        .skip(1)
        .any(|w| w.starts_with('-') && !w.starts_with("--") && w.contains(['r', 'R']));
    let target = first_operand(&words)?;
    if recursive || target.contains('/') && !target.ends_with('/') || !facts.is_dir_here(target) {
        return None;
    }
    fix(
        format!("rm -r {}", words[1..].join(" ")),
        0.8,
        "rm on a directory",
        format!("`{target}` is a directory; `rm` needs `-r` to remove one."),
    )
}

/// `mkdir a/b/c` when `a` does not exist: `-p` creates the parents.
fn mkdir_without_parent(outcome: &CommandOutcome, facts: &Facts) -> Option<Fix> {
    if outcome.status().is_success() || outcome.command().program() != "mkdir" {
        return None;
    }
    let words = outcome.command().words();
    if words.iter().any(|w| *w == "-p" || *w == "--parents") {
        return None;
    }
    let target = first_operand(&words)?;
    let (parent, _) = target.split_once('/')?;
    if parent.is_empty() || parent == "." || parent == ".." || parent == "~" {
        return None;
    }
    if facts.is_dir_here(parent) {
        return None;
    }
    fix(
        format!("mkdir -p {}", words[1..].join(" ")),
        0.9,
        "mkdir without -p",
        format!("`{parent}` does not exist yet; `-p` creates the parents too."),
    )
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
        FixSource::Rule("directory typo".into()),
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
    if !facts.has_program("brew") {
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
        FixSource::Rule("package manager".into()),
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
            docker_desktop: false,
        }
    }
    fn entry(name: &str, is_dir: bool, is_executable: bool) -> DirEntry {
        DirEntry {
            name: name.into(),
            is_dir,
            is_executable,
        }
    }
    fn fixed(text: &str, code: i32, f: &Facts) -> String {
        suggest_fix(&outcome(text, code), f)
            .unwrap_or_else(|| panic!("no fix for `{text}`"))
            .command()
            .as_str()
            .to_string()
    }
    fn none(text: &str, code: i32, f: &Facts) -> bool {
        suggest_fix(&outcome(text, code), f).is_none()
    }

    #[test]
    fn a_python_tool_missing_while_a_venv_is_here_runs_from_the_venv() {
        let mut f = facts(&["pip3", "pipx", "python3"]);
        f.cwd_entries = vec![entry(".venv", true, true), entry("app.py", false, false)];
        let fix = suggest_fix(&outcome("pip install -r requirements.txt", 127), &f).unwrap();
        assert_eq!(
            fix.command().as_str(),
            ".venv/bin/pip install -r requirements.txt"
        );
        assert!(!fix.is_ghostable(), "a guess about the project");
        assert_eq!(fixed("pytest -x", 127, &f), ".venv/bin/pytest -x");
        assert!(none("gti status", 127, &f), "not a Python tool");
        let mut no_venv = facts(&["pip3", "python3"]);
        no_venv.cwd_entries = vec![entry("app.py", false, false)];
        assert_eq!(
            fixed("pip install x", 127, &no_venv),
            "pip3 install x",
            "no venv: the typo guess"
        );
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
        assert_eq!(fix.source(), &FixSource::Rule("command typo".into()));
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
    fn subcommand_typos_are_known_for_the_usual_tools() {
        let f = facts(&["npm", "docker", "brew", "kubectl", "git", "pip3"]);
        assert_eq!(fixed("npm isntall left-pad", 1, &f), "npm install left-pad");
        assert_eq!(fixed("docker psa", 125, &f), "docker ps");
        assert_eq!(fixed("brew instal jq", 1, &f), "brew install jq");
        assert_eq!(fixed("kubectl gte pods", 1, &f), "kubectl get pods");
        assert_eq!(
            fixed("pip3 isntall requests", 1, &f),
            "pip3 install requests"
        );
        assert!(
            none("git ad .", 1, &f),
            "`add` and `am` tie on a 2-letter word"
        );
        assert!(none("docker ./run", 1, &f), "a path is not a subcommand");
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

    #[test]
    fn a_prompt_sign_pasted_with_the_command_is_dropped() {
        let f = facts(&["git"]);
        assert_eq!(fixed("$ git status --short", 127, &f), "git status --short");
        assert!(none("$", 127, &f), "nothing after the sign");
    }

    #[test]
    fn a_no_break_space_becomes_a_space() {
        let f = facts(&["ls"]);
        assert_eq!(fixed("ls\u{a0}-la", 127, &f), "ls -la");
        assert!(none("ls\u{a0}-la", 0, &f), "it ran fine");
    }

    #[test]
    fn a_trailing_cedilla_is_removed() {
        let f = facts(&["ls", "git"]);
        assert_eq!(fixed("lsç", 127, &f), "ls");
        assert_eq!(fixed("git statusç", 1, &f), "git status");
    }

    #[test]
    fn cd_dot_dot_gets_its_space() {
        assert_eq!(fixed("cd..", 127, &facts(&["cd"])), "cd ..");
    }

    #[test]
    fn a_hyphen_between_a_program_and_its_subcommand_becomes_a_space() {
        let f = facts(&["git", "git-lfs", "apt"]);
        assert_eq!(fixed("git-log --oneline", 127, &f), "git log --oneline");
        assert_eq!(fixed("apt-install jq", 127, &f), "apt install jq");
        assert!(
            none("git-lgo", 127, &facts(&["git"])),
            "not a subcommand either"
        );
        assert!(none("npm-install", 127, &f), "npm is not installed here");
    }

    #[test]
    fn a_program_glued_to_its_subcommand_is_split() {
        let f = facts(&["git", "npm", "go"]);
        assert_eq!(
            fixed("gitpush origin main", 127, &f),
            "git push origin main"
        );
        assert_eq!(fixed("npminstall", 127, &f), "npm install");
        assert!(none("goto", 127, &f), "`to` is not a go subcommand");
    }

    #[test]
    fn man_glued_to_its_page_is_split() {
        let f = facts(&["man", "diff"]);
        assert_eq!(fixed("mandiff", 127, &f), "man diff");
        assert!(none("manage", 127, &f), "`age` is not a program");
    }

    #[test]
    fn gradle_becomes_the_wrapper_when_the_project_ships_one() {
        let mut f = facts(&["java"]);
        f.cwd_entries = vec![entry("gradlew", false, true)];
        assert_eq!(fixed("gradle build", 127, &f), "./gradlew build");
        f.cwd_entries = vec![];
        assert!(none("gradle build", 127, &f));
    }

    #[test]
    fn a_script_that_is_here_but_not_executable_gets_its_interpreter() {
        let mut f = facts(&["python3", "python", "node"]);
        f.cwd_entries = vec![entry("app.py", false, false), entry("run.sh", false, false)];
        assert_eq!(fixed("app.py --debug", 127, &f), "python3 app.py --debug");
        assert!(none("run.sh", 127, &f), "no shell on this odd PATH");
        f.cwd_entries = vec![entry("app.py", false, true)];
        assert_eq!(
            fixed("app.py", 127, &f),
            "./app.py",
            "executable: ./ is enough"
        );
    }

    #[test]
    fn a_program_typed_twice_is_said_once() {
        let f = facts(&["git"]);
        assert_eq!(fixed("git git push", 1, &f), "git push");
        assert!(none("echo echo", 1, &f), "not a program we know");
    }

    #[test]
    fn a_git_option_with_one_dash_gets_its_second() {
        let f = facts(&["git"]);
        let fix = suggest_fix(&outcome("git commit -amend", 1), &f).unwrap();
        assert_eq!(fix.command().as_str(), "git commit --amend");
        assert!(!fix.is_ghostable(), "a guess about intent, not pre-typed");
        assert_eq!(
            fixed("git rebase -continue", 128, &f),
            "git rebase --continue"
        );
        assert!(none("git commit -am fix", 1, &f), "`-am` is `-a -m`");
        assert!(none("git commit --amend", 1, &f), "already two dashes");
    }

    #[test]
    fn a_source_file_named_without_its_extension_gets_it() {
        let mut f = facts(&["python", "go"]);
        f.cwd_entries = vec![
            entry("app.py", false, false),
            entry("main.go", false, false),
        ];
        assert_eq!(fixed("python app", 2, &f), "python app.py");
        assert_eq!(fixed("go run main", 1, &f), "go run main.go");
        assert!(none("python -m app", 2, &f), "a flag");
        f.cwd_entries.push(entry("app", true, true));
        assert!(
            none("python app", 2, &f),
            "`app` exists: something else failed"
        );
    }

    #[test]
    fn a_file_refused_for_its_execute_bit_gets_one() {
        let mut f = facts(&["sh"]);
        f.cwd_entries = vec![entry("deploy.sh", false, false)];
        let fix = suggest_fix(&outcome("./deploy.sh --prod", 126), &f).unwrap();
        assert_eq!(
            fix.command().as_str(),
            "chmod +x deploy.sh && ./deploy.sh --prod"
        );
        assert_eq!(fix.danger(), &Danger::None);
        f.cwd_entries = vec![entry("deploy.sh", false, true)];
        assert!(
            none("./deploy.sh", 126, &f),
            "executable: 126 is something else"
        );
    }

    #[test]
    fn cat_on_a_directory_becomes_ls() {
        let mut f = facts(&["cat", "ls"]);
        f.cwd_entries = vec![entry("src", true, true)];
        assert_eq!(fixed("cat src", 1, &f), "ls src");
        assert_eq!(fixed("cat -n src/", 1, &f), "ls -n src/");
        assert!(none("cat src/main.rs", 1, &f), "a path inside");
    }

    #[test]
    fn rm_on_a_directory_gets_dash_r_and_the_red_line_that_goes_with_it() {
        let mut f = facts(&["rm"]);
        f.cwd_entries = vec![entry("build", true, true)];
        let fix = suggest_fix(&outcome("rm build", 1), &f).unwrap();
        assert_eq!(fix.command().as_str(), "rm -r build");
        assert!(matches!(fix.danger(), Danger::Destructive(_)));
        assert!(!fix.is_ghostable(), "never pre-typed");
        assert_eq!(fixed("rm -f build", 1, &f), "rm -r -f build");
        assert!(none("rm -rf build", 1, &f), "already recursive");
    }

    #[test]
    fn mkdir_of_a_nested_path_gets_dash_p() {
        let mut f = facts(&["mkdir"]);
        assert_eq!(fixed("mkdir a/b/c", 1, &f), "mkdir -p a/b/c");
        assert!(none("mkdir -p a/b/c", 1, &f), "already there");
        assert!(
            none("mkdir /a/b", 1, &f),
            "an absolute path: not decidable from here"
        );
        f.cwd_entries = vec![entry("a", true, true)];
        assert!(
            none("mkdir a/b/c", 1, &f),
            "`a` exists: something else failed"
        );
    }
}

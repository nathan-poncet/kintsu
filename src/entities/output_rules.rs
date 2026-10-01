//! The rules that read the command's output, tried once the terminal gave
//! it and before any model is asked. The output is data: patterns are
//! looked for in it, and a word taken from it goes back into a command
//! only when it is a plain path, branch, host or package name. Most of
//! these follow rules of thefuck (MIT), redone here.

use crate::entities::{
    CommandLine, CommandOutcome, Confidence, Facts, Fix, FixSource, Os, distance::closest,
};

type OutputRule = fn(&CommandOutcome, &Facts, &str) -> Option<Fix>;

/// The rules, in the order they are tried; the first fix wins. The ones
/// about one tool's own words come first, the general ones (`sudo`) last.
pub fn suggest_fix_from_output(
    outcome: &CommandOutcome,
    facts: &Facts,
    output: &str,
) -> Option<Fix> {
    if outcome.status().is_success() || output.trim().is_empty() {
        return None;
    }
    let rules: &[OutputRule] = &[
        git_option_said_with_two_dashes,
        git_push_sets_upstream,
        git_pull_sets_upstream,
        git_push_rejected_pulls_first,
        git_stashes_first,
        git_branch_exists,
        git_main_or_master,
        git_untracked_file,
        git_new_branch,
        git_commit_nothing_staged,
        git_merge_unrelated_histories,
        git_rebase_skips,
        git_rm_recursive,
        git_rm_keeps_the_file,
        did_you_mean,
        pip_externally_managed,
        pip_installs_for_the_user,
        python_module_missing,
        docker_daemon_down,
        git_dubious_ownership,
        ssh_key_not_taken,
        linker_cc_missing,
        mkdir_missing_parent,
        destination_directory_missing,
        rm_on_a_directory,
        grep_on_a_directory,
        cp_on_a_directory,
        ssh_host_key_changed,
        long_help,
        without_sudo,
        with_sudo,
    ];
    rules.iter().find_map(|rule| rule(outcome, facts, output))
}

fn fix(command: String, confidence: f32, rule: &str, rationale: String) -> Option<Fix> {
    Some(Fix::new(
        CommandLine::new(command).ok()?,
        Confidence::new(confidence),
        FixSource::Rule(rule.into()),
        rationale,
    ))
}

/// The text between `start` and the next `end`, when both are there.
fn between<'a>(text: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let (_, after) = text.split_once(start)?;
    let (found, _) = after.split_once(end)?;
    Some(found)
}

/// A word from the output that may go back into a command line: a path, a
/// branch, a host, a package; nothing the shell would read as syntax.
fn safe_token(token: &str) -> Option<&str> {
    let token = token.trim();
    let plain = !token.is_empty()
        && token.len() <= 200
        && !token.starts_with('-')
        && token
            .chars()
            .all(|c| c.is_alphanumeric() || "._-/@:+~=,".contains(c));
    plain.then_some(token)
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| haystack.contains(n))
}

/// The words with `extra` inserted right after the word at `index`.
fn insert_after(words: &[&str], index: usize, extra: &str) -> String {
    let mut out: Vec<&str> = words[..=index].to_vec();
    out.push(extra);
    out.extend_from_slice(&words[index + 1..]);
    out.join(" ")
}

fn replace_word(words: &[&str], index: usize, with: &str) -> String {
    let mut out = words.to_vec();
    out[index] = with;
    out.join(" ")
}

/// Whether a short flag letter or its long form is on the line, `-rf`
/// counting for `-r`.
fn has_flag(words: &[&str], short: char, long: &str) -> bool {
    words
        .iter()
        .skip(1)
        .any(|w| *w == long || (w.starts_with('-') && !w.starts_with("--") && w.contains(short)))
}

fn position(words: &[&str], word: &str) -> Option<usize> {
    words.iter().position(|w| *w == word)
}

fn is_git(outcome: &CommandOutcome) -> bool {
    outcome.command().program() == "git"
}

/// git: "error: did you mean `--amend` (with two dashes)".
fn git_option_said_with_two_dashes(
    outcome: &CommandOutcome,
    _: &Facts,
    output: &str,
) -> Option<Fix> {
    if !is_git(outcome) || !output.contains("(with two dashes)") {
        return None;
    }
    let long = between(output, "did you mean `", "`")?;
    let short = long.strip_prefix('-')?;
    let words = outcome.command().words();
    let index = position(&words, short)?;
    fix(
        replace_word(&words, index, long),
        0.9,
        "one dash",
        format!("`{short}` reads as a string of short options; git itself says `{long}`."),
    )
}

/// git push without an upstream: git prints the command to run.
fn git_push_sets_upstream(outcome: &CommandOutcome, _: &Facts, output: &str) -> Option<Fix> {
    if !is_git(outcome) {
        return None;
    }
    let words = outcome.command().words();
    let push = position(&words, "push")?;
    let said = output.split_once("git push --set-upstream ")?.1;
    let mut tokens = said.split_whitespace();
    let remote = safe_token(tokens.next()?)?;
    let branch = safe_token(tokens.next()?)?;
    let mut fixed: Vec<&str> = words[..=push].to_vec();
    fixed.extend(["--set-upstream", remote, branch]);
    fixed.extend(
        words[push + 1..]
            .iter()
            .filter(|w| w.starts_with('-') && !matches!(**w, "-u" | "--set-upstream"))
            .copied(),
    );
    fix(
        fixed.join(" "),
        0.9,
        "push upstream",
        format!("`{branch}` has no upstream yet; git says which to set."),
    )
}

/// git pull on a branch without tracking information.
fn git_pull_sets_upstream(outcome: &CommandOutcome, _: &Facts, output: &str) -> Option<Fix> {
    if !is_git(outcome) || !output.contains("no tracking information") {
        return None;
    }
    let said = output.split_once("--set-upstream-to=")?.1;
    let local = safe_token(said.split_whitespace().nth(1)?)?;
    fix(
        format!(
            "git branch --set-upstream-to=origin/{local} {local} && {}",
            outcome.command().as_str()
        ),
        0.8,
        "pull upstream",
        format!("`{local}` tracks no remote branch yet; `origin/{local}` is the usual one."),
    )
}

/// A push the remote rejected because it has commits you do not.
fn git_push_rejected_pulls_first(outcome: &CommandOutcome, _: &Facts, output: &str) -> Option<Fix> {
    if !is_git(outcome) || !outcome.command().words().contains(&"push") {
        return None;
    }
    let rejected = output.contains("[rejected]")
        && output.contains("failed to push some refs")
        && contains_any(output, &["is behind", "remote contains work"]);
    if !rejected {
        return None;
    }
    fix(
        format!("git pull && {}", outcome.command().as_str()),
        0.75,
        "pull first",
        "The remote has commits you do not have yet; pull them, then push.".into(),
    )
}

/// "Please commit your changes or stash them before you switch branches."
fn git_stashes_first(outcome: &CommandOutcome, _: &Facts, output: &str) -> Option<Fix> {
    if !is_git(outcome) || !output.contains("commit your changes or stash them") {
        return None;
    }
    fix(
        format!(
            "git stash && {} && git stash pop",
            outcome.command().as_str()
        ),
        0.8,
        "stash first",
        "Your local changes are in the way: put them aside, run it, take them back.".into(),
    )
}

/// "fatal: a branch named 'x' already exists."
fn git_branch_exists(outcome: &CommandOutcome, _: &Facts, output: &str) -> Option<Fix> {
    if !is_git(outcome) || !output.contains("already exists") {
        return None;
    }
    let branch = safe_token(between(output, "branch named '", "'")?)?;
    let words = outcome.command().words();
    let command = match words.get(1).copied() {
        Some("switch") => format!("git switch {branch}"),
        Some("checkout" | "branch") => format!("git checkout {branch}"),
        _ => return None,
    };
    fix(
        command,
        0.8,
        "branch exists",
        format!("`{branch}` already exists; switch to it."),
    )
}

/// `master` on a repository whose branch is `main`, and the other way.
fn git_main_or_master(outcome: &CommandOutcome, _: &Facts, output: &str) -> Option<Fix> {
    let unknown = contains_any(
        output,
        &[
            "did not match any file(s) known to git",
            "invalid reference",
            "couldn't find remote ref",
            "unknown revision",
        ],
    );
    if !is_git(outcome) || !unknown {
        return None;
    }
    let words = outcome.command().words();
    let named = |branch: &str| {
        output.contains(&format!("'{branch}'"))
            || output.contains(&format!("ref {branch}"))
            || output.contains(&format!("reference: {branch}"))
    };
    let (index, wrong, right) = words
        .iter()
        .enumerate()
        .skip(1)
        .find_map(|(i, w)| match *w {
            "master" if named("master") => Some((i, "master", "main")),
            "main" if named("main") => Some((i, "main", "master")),
            _ => None,
        })?;
    fix(
        replace_word(&words, index, right),
        0.85,
        "main or master",
        format!("This repository has no `{wrong}`; its branch is `{right}`."),
    )
}

/// A pathspec that matches nothing git knows, though the file is here:
/// git asks whether you forgot to add it.
fn git_untracked_file(outcome: &CommandOutcome, _: &Facts, output: &str) -> Option<Fix> {
    if !is_git(outcome) || !output.contains("Did you forget to 'git add'?") {
        return None;
    }
    let words = outcome.command().words();
    if matches!(
        words.get(1).copied(),
        Some("rm" | "checkout" | "switch" | "restore")
    ) {
        return None;
    }
    let file = safe_token(between(output, "pathspec '", "'")?)?;
    fix(
        format!("git add -- {file} && {}", outcome.command().as_str()),
        0.7,
        "untracked file",
        format!("`{file}` is not tracked yet; git says so."),
    )
}

/// `git checkout x` when `x` is neither a branch nor a file: perhaps a
/// branch to create.
fn git_new_branch(outcome: &CommandOutcome, _: &Facts, output: &str) -> Option<Fix> {
    if !is_git(outcome) || !output.contains("did not match any file(s) known to git") {
        return None;
    }
    let words = outcome.command().words();
    let name = safe_token(between(output, "pathspec '", "'")?)?;
    let command = match (words.get(1).copied(), words.get(2).copied(), words.len()) {
        (Some("checkout"), Some(n), 3) if n == name => format!("git checkout -b {name}"),
        (Some("switch"), Some(n), 3) if n == name => format!("git switch -c {name}"),
        _ => return None,
    };
    fix(
        command,
        0.6,
        "new branch",
        format!("No branch or file named `{name}`; create the branch, if that was the intent."),
    )
}

/// "no changes added to commit": nothing was staged.
fn git_commit_nothing_staged(outcome: &CommandOutcome, _: &Facts, output: &str) -> Option<Fix> {
    if !is_git(outcome) || !output.contains("no changes added to commit") {
        return None;
    }
    let words = outcome.command().words();
    let commit = position(&words, "commit")?;
    if has_flag(&words, 'a', "--all") {
        return None;
    }
    fix(
        insert_after(&words, commit, "-a"),
        0.7,
        "nothing staged",
        "Nothing is staged; `-a` commits every tracked change.".into(),
    )
}

/// "fatal: refusing to merge unrelated histories".
fn git_merge_unrelated_histories(outcome: &CommandOutcome, _: &Facts, output: &str) -> Option<Fix> {
    if !is_git(outcome) || !output.contains("refusing to merge unrelated histories") {
        return None;
    }
    fix(
        format!("{} --allow-unrelated-histories", outcome.command().as_str()),
        0.85,
        "unrelated histories",
        "The two histories share no commit; git wants you to say so.".into(),
    )
}

/// `git rebase --continue` when the commit has nothing left to apply.
fn git_rebase_skips(outcome: &CommandOutcome, _: &Facts, output: &str) -> Option<Fix> {
    if !is_git(outcome) || !output.contains("nothing left to stage") {
        return None;
    }
    let words = outcome.command().words();
    let index = position(&words, "--continue")?;
    fix(
        replace_word(&words, index, "--skip"),
        0.8,
        "rebase skip",
        "This commit has nothing left to apply; skip it.".into(),
    )
}

/// "fatal: not removing 'x' recursively without -r".
fn git_rm_recursive(outcome: &CommandOutcome, _: &Facts, output: &str) -> Option<Fix> {
    if !is_git(outcome) || !output.contains("recursively without -r") {
        return None;
    }
    let words = outcome.command().words();
    let rm = position(&words, "rm")?;
    fix(
        insert_after(&words, rm, "-r"),
        0.9,
        "git rm -r",
        "It is a directory; `git rm` needs `-r` for one.".into(),
    )
}

/// `git rm x` on a file with changes: `--cached` keeps it on disk.
fn git_rm_keeps_the_file(outcome: &CommandOutcome, _: &Facts, output: &str) -> Option<Fix> {
    if !is_git(outcome) || !output.contains("use --cached to keep the file") {
        return None;
    }
    let words = outcome.command().words();
    let rm = position(&words, "rm")?;
    if words.contains(&"--cached") {
        return None;
    }
    fix(
        insert_after(&words, rm, "--cached"),
        0.75,
        "keep the file",
        "The file has changes; `--cached` unstages it and keeps it on disk. `-f` would discard them.".into(),
    )
}

/// The phrases a tool uses to say the subcommand is unknown, so that a
/// compiler's "did you mean" about a variable is left alone.
const UNKNOWN_COMMAND: &[&str] = &[
    "unknown command",
    "no such command",
    "unknown subcommand",
    "missing script",
    "has no command named",
    "is not a git command",
    "is not a docker command",
    "is not a valid command",
    "invalid choice:",
    "command not found",
    "not found. did you mean",
    "unrecognized command",
];

/// The suggestion a tool prints, in the forms git, cargo, npm, gh,
/// kubectl, pip, yarn, gem, terraform and hg use.
fn suggested_in(output: &str, wrong: &str) -> Option<String> {
    for marker in ["Did you mean this?", "did you mean this?"] {
        if let Some((_, after)) = output.split_once(marker) {
            let line = after.lines().map(str::trim).find(|l| !l.is_empty())?;
            let line = line.split(" #").next()?.trim();
            return Some(line.to_string());
        }
    }
    for marker in ["most similar command is", "most similar commands are"] {
        if let Some((_, after)) = output.split_once(marker) {
            let candidates: Vec<&str> = after
                .lines()
                .skip(1)
                .map(str::trim)
                .take_while(|l| !l.is_empty())
                .collect();
            return pick(wrong, &candidates).map(str::to_string);
        }
    }
    for marker in [
        "did you mean one of ",
        "Did you mean one of ",
        "did you mean ",
        "Did you mean ",
        "Did you mean? ",
        "maybe you meant ",
        "similar name exists: ",
    ] {
        let Some((_, after)) = output.split_once(marker) else {
            continue;
        };
        let after = after.trim_start();
        let quoted = ['`', '"', '\''].iter().find(|q| after.starts_with(**q));
        let said = match quoted {
            Some(q) => between(after, &q.to_string(), &q.to_string())?,
            None => after.split(['?', ')', '\n']).next()?.trim(),
        };
        let candidates: Vec<&str> = said.split(", ").map(str::trim).collect();
        return pick(wrong, &candidates).map(str::to_string);
    }
    None
}

/// One candidate, or the one closest to the word typed.
fn pick<'a>(wrong: &str, candidates: &[&'a str]) -> Option<&'a str> {
    match candidates {
        [] => None,
        [only] => Some(only),
        many => closest(wrong, many.iter().copied(), 3),
    }
}

/// npm prefixes every line of an error with `npm ERR!` or `npm error`.
fn without_npm_prefix(output: &str) -> String {
    output
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            ["npm ERR!", "npm error", "npm WARN", "npm warn"]
                .iter()
                .find_map(|prefix| trimmed.strip_prefix(prefix))
                .unwrap_or(line)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A tool that names the subcommand it does not know, and the one it
/// thinks was meant: its own guess beats our tables.
fn did_you_mean(outcome: &CommandOutcome, _: &Facts, output: &str) -> Option<Fix> {
    let output = &without_npm_prefix(output);
    let lower = output.to_lowercase();
    if !contains_any(&lower, UNKNOWN_COMMAND) {
        return None;
    }
    let words = outcome.command().words();
    let program = *words.first()?;
    let quoted = |w: &str| {
        output.contains(&format!("'{w}'"))
            || output.contains(&format!("\"{w}\""))
            || output.contains(&format!("`{w}`"))
    };
    let index = words
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, w)| !w.starts_with('-') && quoted(w))
        .map(|(i, _)| i)
        .or_else(|| (words.len() > 1 && !words[1].starts_with('-')).then_some(1))?;
    let wrong = words[index];
    let suggestion = suggested_in(output, wrong)?;
    let said: Vec<&str> = suggestion
        .split_whitespace()
        .map(safe_token)
        .collect::<Option<Vec<&str>>>()?;
    if said.is_empty() || said == [wrong] {
        return None;
    }
    let command = if said.first() == Some(&program) {
        let mut fixed = said.clone();
        fixed.extend_from_slice(&words[index + 1..]);
        fixed.join(" ")
    } else if said.len() == 1 {
        replace_word(&words, index, said[0])
    } else {
        return None;
    };
    fix(
        command,
        0.85,
        "did you mean",
        format!("`{program}` itself suggests `{}`.", said.join(" ")),
    )
}

/// pip refused by a Python the system manages (PEP 668: Debian 12,
/// Homebrew's): a project virtualenv takes the packages, the one here when
/// there is one. `--break-system-packages` is what pip itself warns against.
fn pip_externally_managed(outcome: &CommandOutcome, facts: &Facts, output: &str) -> Option<Fix> {
    if !output.contains("externally-managed-environment") {
        return None;
    }
    let words = outcome.command().words();
    if !matches!(words.first().copied(), Some("pip" | "pip3")) {
        return None;
    }
    let install = position(&words, "install")?;
    let packages = words[install + 1..].join(" ");
    if packages.is_empty() {
        return None;
    }
    if facts.is_dir_here(".venv") {
        return fix(
            format!(".venv/bin/pip install {packages}"),
            0.7,
            "externally managed",
            "This Python is managed by the system (PEP 668); the project's `.venv` takes the packages.".into(),
        );
    }
    fix(
        format!("python3 -m venv .venv && .venv/bin/pip install {packages}"),
        0.7,
        "externally managed",
        "This Python is managed by the system (PEP 668); a project virtualenv takes the packages. For a tool you run, `pipx install` instead.".into(),
    )
}

/// `pip install` refused for the system's site-packages.
fn pip_installs_for_the_user(outcome: &CommandOutcome, _: &Facts, output: &str) -> Option<Fix> {
    let words = outcome.command().words();
    if !matches!(words.first().copied(), Some("pip" | "pip3")) || words.contains(&"--user") {
        return None;
    }
    let install = position(&words, "install")?;
    if !contains_any(
        output,
        &["Permission denied", "EnvironmentError", "Errno 13"],
    ) {
        return None;
    }
    fix(
        insert_after(&words, install, "--user"),
        0.8,
        "pip --user",
        "The system's site-packages are not writable; `--user` installs for you.".into(),
    )
}

/// Modules whose package on PyPI has another name.
const PYPI_NAMES: &[(&str, &str)] = &[
    ("Crypto", "pycryptodome"),
    ("MySQLdb", "mysqlclient"),
    ("OpenSSL", "pyOpenSSL"),
    ("PIL", "pillow"),
    ("attr", "attrs"),
    ("bs4", "beautifulsoup4"),
    ("cv2", "opencv-python"),
    ("dateutil", "python-dateutil"),
    ("dotenv", "python-dotenv"),
    ("git", "GitPython"),
    ("gi", "PyGObject"),
    ("jwt", "PyJWT"),
    ("psycopg2", "psycopg2-binary"),
    ("serial", "pyserial"),
    ("sklearn", "scikit-learn"),
    ("usb", "pyusb"),
    ("yaml", "pyyaml"),
];

/// "ModuleNotFoundError: No module named 'x'".
fn python_module_missing(outcome: &CommandOutcome, _: &Facts, output: &str) -> Option<Fix> {
    let module = between(output, "No module named '", "'").or_else(|| {
        output
            .split_once("No module named ")?
            .1
            .split_whitespace()
            .next()
    })?;
    let top = safe_token(module.split('.').next()?)?;
    let package = PYPI_NAMES
        .iter()
        .find(|(m, _)| *m == top)
        .map_or(top, |(_, p)| p);
    let program = outcome.command().program();
    let installer = match program {
        "python" | "python3" => format!("{program} -m pip install {package}"),
        _ => format!("pip install {package}"),
    };
    fix(
        format!("{installer} && {}", outcome.command().as_str()),
        0.7,
        "missing module",
        format!("`{top}` is not installed for this Python; `{package}` is its package."),
    )
}

/// "Cannot connect to the Docker daemon": nothing listens on its socket;
/// start whatever runs it on this machine. Docker Desktop takes a while to
/// come up, so the command is not chained after it.
fn docker_daemon_down(outcome: &CommandOutcome, facts: &Facts, output: &str) -> Option<Fix> {
    if !output.contains("Cannot connect to the Docker daemon") {
        return None;
    }
    let command = outcome.command().as_str();
    if facts.os == Some(Os::Mac) && facts.docker_desktop {
        return fix(
            "open -a Docker".into(),
            0.75,
            "docker daemon",
            "The Docker daemon is not running; Docker Desktop starts it. Run the command again once it is up.".into(),
        );
    }
    if facts.has_program("colima") {
        return fix(
            format!("colima start && {command}"),
            0.75,
            "docker daemon",
            "The Docker daemon is not running; `colima start` brings it up.".into(),
        );
    }
    if facts.os == Some(Os::Linux) {
        return fix(
            format!("sudo systemctl start docker && {command}"),
            0.75,
            "docker daemon",
            "The Docker daemon is not running; systemd starts it.".into(),
        );
    }
    None
}

/// git refuses a repository another user owns, and prints the exception
/// to add. The path goes back into the command only as a plain path.
fn git_dubious_ownership(outcome: &CommandOutcome, _: &Facts, output: &str) -> Option<Fix> {
    if !output.contains("detected dubious ownership in repository at '") {
        return None;
    }
    let dir = safe_token(between(
        output,
        "detected dubious ownership in repository at '",
        "'",
    )?)?;
    fix(
        format!(
            "git config --global --add safe.directory {dir} && {}",
            outcome.command().as_str()
        ),
        0.75,
        "dubious ownership",
        format!(
            "`{dir}` belongs to another user; git refuses it until you say you trust that directory."
        ),
    )
}

/// "Permission denied (publickey)": the server took none of the keys the
/// agent offered. Loading the default key is the usual cure; a key that
/// was never set up for this host is the other cause, said in the rationale.
fn ssh_key_not_taken(outcome: &CommandOutcome, facts: &Facts, output: &str) -> Option<Fix> {
    if !output.contains("Permission denied (publickey") {
        return None;
    }
    let add = if facts.os == Some(Os::Mac) {
        "ssh-add --apple-use-keychain"
    } else {
        "ssh-add"
    };
    fix(
        format!("{add} && {}", outcome.command().as_str()),
        0.6,
        "ssh key",
        "The server took none of the keys the agent offered; `ssh-add` loads your default key. If no key is set up for this host, that is the cause.".into(),
    )
}

/// rustc, or any C build, without a linker: the system's build tools are
/// missing. A link that failed for a missing library is another story and
/// is left alone.
fn linker_cc_missing(outcome: &CommandOutcome, facts: &Facts, output: &str) -> Option<Fix> {
    let no_linker = output.contains("linker `cc` not found")
        || (output.contains("linking with `cc` failed")
            && contains_any(
                output,
                &[
                    "cc: not found",
                    "cc: command not found",
                    "No such file or directory (os error 2)",
                ],
            ));
    if !no_linker {
        return None;
    }
    let command = outcome.command().as_str();
    let install = match facts.os {
        Some(Os::Mac) => "xcode-select --install".to_string(),
        _ if facts.has_program("apt") || facts.has_program("apt-get") => {
            format!("sudo apt install build-essential && {command}")
        }
        _ if facts.has_program("dnf") => {
            format!("sudo dnf groupinstall \"Development Tools\" && {command}")
        }
        _ => return None,
    };
    fix(
        install,
        0.75,
        "no linker",
        "No C linker (`cc`) is installed; the system's build tools provide one.".into(),
    )
}

/// `mkdir a/b/c` when a parent is missing.
fn mkdir_missing_parent(outcome: &CommandOutcome, _: &Facts, output: &str) -> Option<Fix> {
    let words = outcome.command().words();
    if words.first() != Some(&"mkdir") || words.iter().any(|w| matches!(*w, "-p" | "--parents")) {
        return None;
    }
    if !output.contains("No such file or directory") {
        return None;
    }
    fix(
        format!("mkdir -p {}", words[1..].join(" ")),
        0.9,
        "mkdir without -p",
        "A parent directory does not exist yet; `-p` creates the parents too.".into(),
    )
}

/// `cp`, `mv` or `touch` into a directory that does not exist yet. A
/// source that is missing is another story: GNU says "cannot stat", and
/// the working directory tells when it is known.
fn destination_directory_missing(
    outcome: &CommandOutcome,
    facts: &Facts,
    output: &str,
) -> Option<Fix> {
    let words = outcome.command().words();
    if !matches!(words.first().copied(), Some("cp" | "mv" | "touch")) {
        return None;
    }
    if output.contains("cannot stat")
        || !contains_any(
            output,
            &[
                "No such file or directory",
                "Not a directory",
                "does not exist",
            ],
        )
    {
        return None;
    }
    let operands: Vec<&str> = words
        .iter()
        .skip(1)
        .filter(|w| !w.starts_with('-'))
        .copied()
        .collect();
    let destination = *operands.last()?;
    if operands.len() > 1
        && let Some(source) = operands.first()
        && !source.contains('/')
        && !facts.cwd_entries.is_empty()
        && facts.cwd_entries.iter().all(|e| e.name != *source)
    {
        return None;
    }
    let (dir, _) = destination.rsplit_once('/')?;
    let dir = safe_token(dir)?;
    if !output.contains(dir) {
        return None;
    }
    fix(
        format!("mkdir -p {dir} && {}", outcome.command().as_str()),
        0.75,
        "missing directory",
        format!("`{dir}` does not exist yet."),
    )
}

/// `rm dir`: "is a directory".
fn rm_on_a_directory(outcome: &CommandOutcome, _: &Facts, output: &str) -> Option<Fix> {
    let words = outcome.command().words();
    if words.first() != Some(&"rm")
        || has_flag(&words, 'r', "--recursive")
        || has_flag(&words, 'R', "")
    {
        return None;
    }
    if !output.to_lowercase().contains("is a directory") {
        return None;
    }
    fix(
        format!("rm -r {}", words[1..].join(" ")),
        0.8,
        "rm on a directory",
        "It is a directory; `rm` needs `-r` to remove one.".into(),
    )
}

/// `grep x dir`: "Is a directory".
fn grep_on_a_directory(outcome: &CommandOutcome, _: &Facts, output: &str) -> Option<Fix> {
    let words = outcome.command().words();
    if !matches!(words.first().copied(), Some("grep" | "egrep" | "fgrep")) {
        return None;
    }
    if has_flag(&words, 'r', "--recursive") || has_flag(&words, 'R', "--dereference-recursive") {
        return None;
    }
    if !output.contains("Is a directory") {
        return None;
    }
    fix(
        insert_after(&words, 0, "-r"),
        0.85,
        "grep -r",
        "It is a directory; `-r` searches inside.".into(),
    )
}

/// `cp dir elsewhere`: "omitting directory".
fn cp_on_a_directory(outcome: &CommandOutcome, _: &Facts, output: &str) -> Option<Fix> {
    let words = outcome.command().words();
    if words.first() != Some(&"cp")
        || has_flag(&words, 'r', "--recursive")
        || has_flag(&words, 'R', "")
        || has_flag(&words, 'a', "--archive")
    {
        return None;
    }
    if !contains_any(
        output,
        &["omitting directory", "is a directory (not copied)"],
    ) {
        return None;
    }
    fix(
        insert_after(&words, 0, "-r"),
        0.9,
        "cp -r",
        "It is a directory; `cp` needs `-r` to copy one.".into(),
    )
}

/// ssh: "REMOTE HOST IDENTIFICATION HAS CHANGED".
fn ssh_host_key_changed(outcome: &CommandOutcome, _: &Facts, output: &str) -> Option<Fix> {
    if !output.contains("REMOTE HOST IDENTIFICATION HAS CHANGED") {
        return None;
    }
    let host = safe_token(between(output, "Host key for ", " has changed")?)?;
    fix(
        format!("ssh-keygen -R {host} && {}", outcome.command().as_str()),
        0.7,
        "host key changed",
        format!(
            "The key of `{host}` changed. If you did not expect that, stop here: this is what an interception looks like."
        ),
    )
}

/// `-h` where only `--help` is understood.
fn long_help(outcome: &CommandOutcome, _: &Facts, output: &str) -> Option<Fix> {
    let words = outcome.command().words();
    let index = position(&words, "-h")?;
    let lower = output.to_lowercase();
    if !contains_any(
        &lower,
        &[
            "invalid option",
            "illegal option",
            "unrecognized option",
            "unknown option",
        ],
    ) {
        return None;
    }
    fix(
        replace_word(&words, index, "--help"),
        0.8,
        "long help",
        "`-h` is not an option here; `--help` is.".into(),
    )
}

const NOT_AS_ROOT: &[&str] = &[
    "you cannot perform this operation as root",
    "must not be run as root",
    "should not be run as root",
    "do not run as root",
    "don't run this as root",
    "running as root is not supported",
    "refusing to run as root",
];

/// `sudo x` when x refuses to run as root.
fn without_sudo(outcome: &CommandOutcome, _: &Facts, output: &str) -> Option<Fix> {
    let words = outcome.command().words();
    if words.first() != Some(&"sudo") || words.len() < 2 {
        return None;
    }
    if !contains_any(&output.to_lowercase(), NOT_AS_ROOT) {
        return None;
    }
    fix(
        words[1..].join(" "),
        0.85,
        "not as root",
        format!("`{}` refuses to run as root.", words[1]),
    )
}

const NEEDS_ROOT: &[&str] = &[
    "permission denied",
    "eacces",
    "pkg: insufficient privileges",
    "you cannot perform this operation unless you are root",
    "non-root users cannot",
    "operation not permitted",
    "not super-user",
    "superuser privilege",
    "root privilege",
    "this command has to be run under the root user",
    "this operation requires root",
    "requested operation requires superuser privilege",
    "must be run as root",
    "must run as root",
    "must be superuser",
    "must be root",
    "need to be root",
    "need root",
    "needs to be run as root",
    "only root can",
    "you don't have access to the history db",
    "authentication is required",
    "edspermissionerror",
    "you don't have write permissions",
    "use `sudo`",
    "sudorequirederror",
    "error: insufficient privileges",
    "updatedb: can not open a temporary file",
];

/// A command refused for lack of permission. Not for `git`, whose
/// "Permission denied (publickey)" is about ssh, nor for a file refused
/// as not executable, nor for `cd`.
fn with_sudo(outcome: &CommandOutcome, _: &Facts, output: &str) -> Option<Fix> {
    let program = outcome.command().program();
    if matches!(program, "sudo" | "doas" | "cd" | "git" | "ssh" | "scp")
        || outcome.status().is_not_executable()
    {
        return None;
    }
    let lower = output.to_lowercase();
    if lower.contains("denied (publickey") || !contains_any(&lower, NEEDS_ROOT) {
        return None;
    }
    fix(
        format!("sudo {}", outcome.command().as_str()),
        0.8,
        "needs root",
        format!("`{program}` was refused for lack of permission; `sudo` runs it as root."),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{Danger, ExitStatus};

    fn outcome(text: &str, code: i32) -> CommandOutcome {
        CommandOutcome::new(CommandLine::new(text).unwrap(), ExitStatus::new(code))
    }
    fn fixed(text: &str, code: i32, output: &str) -> Fix {
        fixed_with(text, code, output, &Facts::default())
    }
    fn fixed_with(text: &str, code: i32, output: &str, facts: &Facts) -> Fix {
        suggest_fix_from_output(&outcome(text, code), facts, output)
            .unwrap_or_else(|| panic!("no fix for `{text}` with:\n{output}"))
    }
    fn here(names: &[&str]) -> Facts {
        Facts {
            cwd_entries: names
                .iter()
                .map(|n| crate::entities::DirEntry {
                    name: n.to_string(),
                    is_dir: false,
                    is_executable: false,
                })
                .collect(),
            ..Facts::default()
        }
    }
    fn command(text: &str, code: i32, output: &str) -> String {
        fixed(text, code, output).command().as_str().to_string()
    }
    fn none(text: &str, code: i32, output: &str) -> bool {
        suggest_fix_from_output(&outcome(text, code), &Facts::default(), output).is_none()
    }
    fn machine(os: Os, programs: &[&str], dirs: &[&str], docker_desktop: bool) -> Facts {
        Facts {
            os: Some(os),
            executables: programs.iter().map(|p| p.to_string()).collect(),
            cwd_entries: dirs
                .iter()
                .map(|d| crate::entities::DirEntry {
                    name: d.to_string(),
                    is_dir: true,
                    is_executable: true,
                })
                .collect(),
            docker_desktop,
            aliases: Vec::new(),
        }
    }

    const PEP_668: &str = "error: externally-managed-environment\n\n× This environment is externally managed\n╰─> To install Python packages system-wide, try apt install\n    python3-xyz, where xyz is the package you are trying to\n    install.\n    \n    If you wish to install a non-Debian-packaged Python package,\n    create a virtual environment using python3 -m venv path/to/venv.\n\nnote: If you believe this is a mistake, please contact your Python installation or OS distribution provider. You can override this, at the risk of breaking your Python installation or OS, by passing --break-system-packages.\nhint: See PEP 668 for the detailed specification.";

    #[test]
    fn a_system_managed_python_sends_pip_to_a_virtualenv_the_one_here_first() {
        let fix = fixed("pip install requests", 1, PEP_668);
        assert_eq!(
            fix.command().as_str(),
            "python3 -m venv .venv && .venv/bin/pip install requests"
        );
        assert!(!fix.is_ghostable(), "a guess about the project");
        assert!(
            fix.rationale().contains("pipx"),
            "the tool case is said, not guessed"
        );
        let with_venv = machine(Os::Linux, &["pip3"], &[".venv"], false);
        assert_eq!(
            fixed_with("pip3 install -r requirements.txt", 1, PEP_668, &with_venv)
                .command()
                .as_str(),
            ".venv/bin/pip install -r requirements.txt"
        );
        assert!(none("pip install", 1, PEP_668), "nothing to install");
        assert!(
            none("pip list", 1, PEP_668),
            "only an install is redirected"
        );
    }

    const NO_DOCKER: &str = "Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?";

    #[test]
    fn a_docker_daemon_that_is_down_is_started_the_way_this_machine_runs_it() {
        let desktop = machine(Os::Mac, &["docker"], &[], true);
        assert_eq!(
            fixed_with("docker ps", 1, NO_DOCKER, &desktop)
                .command()
                .as_str(),
            "open -a Docker",
            "Docker Desktop takes a while: the command is not chained"
        );
        let colima = machine(Os::Mac, &["docker", "colima"], &[], false);
        assert_eq!(
            fixed_with("docker compose up -d", 1, NO_DOCKER, &colima)
                .command()
                .as_str(),
            "colima start && docker compose up -d"
        );
        let linux = machine(Os::Linux, &["docker"], &[], false);
        let fix = fixed_with("docker ps", 1, NO_DOCKER, &linux);
        assert_eq!(
            fix.command().as_str(),
            "sudo systemctl start docker && docker ps"
        );
        assert_eq!(fix.danger(), &Danger::NeedsPrivilege);
        let bare_mac = machine(Os::Mac, &["docker"], &[], false);
        assert!(
            suggest_fix_from_output(&outcome("docker ps", 1), &bare_mac, NO_DOCKER).is_none(),
            "neither Docker Desktop nor colima: nothing to start"
        );
    }

    #[test]
    fn a_repository_another_user_owns_is_trusted_by_the_line_git_prints() {
        let out = "fatal: detected dubious ownership in repository at '/srv/app'\nTo add an exception for this directory, call:\n\n\tgit config --global --add safe.directory /srv/app";
        let fix = fixed("git status", 128, out);
        assert_eq!(
            fix.command().as_str(),
            "git config --global --add safe.directory /srv/app && git status"
        );
        assert!(!fix.is_ghostable(), "trust is the user's call");
        assert!(
            none(
                "git status",
                128,
                "fatal: detected dubious ownership in repository at '/srv/$(rm -rf ~)'"
            ),
            "nothing from the output that the shell would read as syntax"
        );
    }

    #[test]
    fn a_key_the_server_took_none_of_is_loaded_into_the_agent_first() {
        let out = "git@github.com: Permission denied (publickey).\nfatal: Could not read from remote repository.";
        let linux = machine(Os::Linux, &["git"], &[], false);
        let fix = fixed_with("git push", 128, out, &linux);
        assert_eq!(fix.command().as_str(), "ssh-add && git push");
        assert!(
            fix.confidence().value() < 0.7,
            "a guess: the key may not exist"
        );
        let mac = machine(Os::Mac, &["ssh"], &[], false);
        assert_eq!(
            fixed_with(
                "ssh deploy@host",
                255,
                "deploy@host: Permission denied (publickey,password).",
                &mac
            )
            .command()
            .as_str(),
            "ssh-add --apple-use-keychain && ssh deploy@host"
        );
    }

    #[test]
    fn a_missing_linker_installs_the_build_tools_a_missing_library_does_not() {
        let no_linker = "error: linker `cc` not found\n  |\n  = note: No such file or directory (os error 2)\n\nerror: could not compile `kintsu` (bin \"kintsu\") due to 1 previous error";
        let mac = machine(Os::Mac, &["cargo"], &[], false);
        assert_eq!(
            fixed_with("cargo build", 101, no_linker, &mac)
                .command()
                .as_str(),
            "xcode-select --install"
        );
        let debian = machine(Os::Linux, &["cargo", "apt"], &[], false);
        assert_eq!(
            fixed_with("cargo build", 101, no_linker, &debian)
                .command()
                .as_str(),
            "sudo apt install build-essential && cargo build"
        );
        let fedora = machine(Os::Linux, &["cargo", "dnf"], &[], false);
        assert_eq!(
            fixed_with("cargo build", 101, no_linker, &fedora)
                .command()
                .as_str(),
            "sudo dnf groupinstall \"Development Tools\" && cargo build"
        );
        let missing_lib = "error: linking with `cc` failed: exit status: 1\n  = note: /usr/bin/ld: cannot find -lssl: No such file or directory\n          collect2: error: ld returned 1 exit status";
        assert!(
            suggest_fix_from_output(&outcome("cargo build", 101), &debian, missing_lib).is_none(),
            "a library is missing, not the linker"
        );
        let no_cc =
            "error: linking with `cc` failed: exit status: 127\n  = note: sh: 1: cc: not found";
        assert_eq!(
            fixed_with("cargo build", 101, no_cc, &debian)
                .command()
                .as_str(),
            "sudo apt install build-essential && cargo build"
        );
    }

    #[test]
    fn a_successful_command_or_an_empty_output_gets_nothing() {
        assert!(none("git push", 0, "Permission denied"));
        assert!(none("git push", 1, "   \n"));
    }

    #[test]
    fn git_says_the_option_wanted_two_dashes() {
        let out = "error: did you mean `--amend` (with two dashes)?";
        assert_eq!(command("git commit -amend", 129, out), "git commit --amend");
    }

    #[test]
    fn a_push_without_upstream_takes_the_command_git_prints() {
        let out = "fatal: The current branch feature/x has no upstream branch.\nTo push the current branch and set the remote as upstream, use\n\n    git push --set-upstream origin feature/x\n";
        assert_eq!(
            command("git push", 128, out),
            "git push --set-upstream origin feature/x"
        );
        assert_eq!(
            command("git push -u --force-with-lease", 128, out),
            "git push --set-upstream origin feature/x --force-with-lease"
        );
        assert!(
            none(
                "git push",
                128,
                "    git push --set-upstream origin $(rm -rf ~)\n"
            ),
            "nothing from the output that the shell would read as syntax"
        );
    }

    #[test]
    fn a_pull_without_tracking_information_sets_it_first() {
        let out = "There is no tracking information for the current branch.\nPlease specify which branch you want to merge with.\nSee git-pull(1) for details.\n\n    git pull <remote> <branch>\n\nIf you wish to set tracking information for this branch you can do so with:\n\n    git branch --set-upstream-to=<remote>/<branch> feature/x\n";
        assert_eq!(
            command("git pull", 1, out),
            "git branch --set-upstream-to=origin/feature/x feature/x && git pull"
        );
    }

    #[test]
    fn a_rejected_push_pulls_first() {
        let out = " ! [rejected]        main -> main (fetch first)\nerror: failed to push some refs to 'github.com:x/y.git'\nhint: Updates were rejected because the remote contains work that you do not\nhint: have locally.";
        assert_eq!(command("git push", 1, out), "git pull && git push");
    }

    #[test]
    fn local_changes_in_the_way_are_stashed_around_the_command() {
        let out = "error: Your local changes to the following files would be overwritten by checkout:\n\tsrc/main.rs\nPlease commit your changes or stash them before you switch branches.\nAborting";
        assert_eq!(
            command("git checkout main", 1, out),
            "git stash && git checkout main && git stash pop"
        );
    }

    #[test]
    fn creating_a_branch_that_exists_switches_to_it() {
        let out = "fatal: a branch named 'feature/x' already exists";
        assert_eq!(
            command("git checkout -b feature/x", 128, out),
            "git checkout feature/x"
        );
        assert_eq!(
            command("git switch -c feature/x", 128, out),
            "git switch feature/x"
        );
        assert_eq!(
            command("git branch feature/x", 128, out),
            "git checkout feature/x"
        );
    }

    #[test]
    fn master_becomes_main_where_main_is_the_branch() {
        let out = "error: pathspec 'master' did not match any file(s) known to git";
        assert_eq!(command("git checkout master", 1, out), "git checkout main");
        let out = "fatal: couldn't find remote ref main";
        assert_eq!(
            command("git pull origin main", 1, out),
            "git pull origin master"
        );
    }

    #[test]
    fn a_file_git_does_not_know_yet_is_added_first() {
        let out = "error: pathspec 'notes.md' did not match any file(s) known to git\nhint: Did you forget to 'git add'?";
        assert_eq!(
            command("git commit notes.md -m x", 1, out),
            "git add -- notes.md && git commit notes.md -m x"
        );
        assert!(
            none("git rm notes.md", 1, out),
            "adding a file to remove it makes no sense"
        );
    }

    #[test]
    fn checking_out_a_name_git_does_not_know_offers_a_new_branch() {
        let out = "error: pathspec 'feature/y' did not match any file(s) known to git";
        let fix = fixed("git checkout feature/y", 1, out);
        assert_eq!(fix.command().as_str(), "git checkout -b feature/y");
        assert!(!fix.is_ghostable(), "a guess about intent");
        assert_eq!(
            command("git switch feature/y", 1, out),
            "git switch -c feature/y"
        );
        assert!(
            none("git checkout feature/y -- src", 1, out),
            "a pathspec after --"
        );
    }

    #[test]
    fn a_commit_with_nothing_staged_gets_dash_a() {
        let out = "no changes added to commit (use \"git add\" and/or \"git commit -a\")";
        assert_eq!(command("git commit -m fix", 1, out), "git commit -a -m fix");
        assert!(none("git commit -am fix", 1, out), "already -a");
    }

    #[test]
    fn unrelated_histories_are_allowed_when_git_refuses() {
        let out = "fatal: refusing to merge unrelated histories";
        assert_eq!(
            command("git pull origin main", 128, out),
            "git pull origin main --allow-unrelated-histories"
        );
    }

    #[test]
    fn a_rebase_with_nothing_left_skips() {
        let out = "No changes - did you forget to use 'git add'?\nIf there is nothing left to stage, chances are that something else\nalready introduced the same changes; you might want to skip this patch.";
        assert_eq!(
            command("git rebase --continue", 1, out),
            "git rebase --skip"
        );
    }

    #[test]
    fn git_rm_on_a_directory_and_on_a_changed_file() {
        let out = "fatal: not removing 'docs' recursively without -r";
        assert_eq!(command("git rm docs", 128, out), "git rm -r docs");
        let out = "error: the following file has local modifications:\n    notes.md\n(use --cached to keep the file, or -f to force removal)";
        assert_eq!(
            command("git rm notes.md", 1, out),
            "git rm --cached notes.md"
        );
    }

    #[test]
    fn a_tools_own_did_you_mean_is_taken_in_every_format() {
        assert_eq!(
            command(
                "git stauts",
                1,
                "git: 'stauts' is not a git command. See 'git --help'.\n\nThe most similar command is\n\tstatus"
            ),
            "git status"
        );
        assert_eq!(
            command(
                "git statu",
                1,
                "git: 'statu' is not a git command. See 'git --help'.\n\nThe most similar commands are\n\tstatus\n\tstash"
            ),
            "git status",
            "the closest of several"
        );
        assert!(
            none(
                "git sta",
                1,
                "git: 'sta' is not a git command. See 'git --help'.\n\nThe most similar commands are\n\tstash\n\tstage"
            ),
            "a tie is refused, as everywhere"
        );
        assert_eq!(
            command(
                "cargo biuld --release",
                101,
                "error: no such command: `biuld`\n\n\tDid you mean `build`?"
            ),
            "cargo build --release"
        );
        assert_eq!(
            command(
                "npm isntall left-pad",
                1,
                "Unknown command: \"isntall\"\n\nDid you mean this?\n    npm install # Install a package\n\nTo see a list of supported npm commands, run:\n  npm help"
            ),
            "npm install left-pad"
        );
        assert_eq!(
            command(
                "npm run tets",
                1,
                "npm ERR! Missing script: \"tets\"\nnpm ERR!\nnpm ERR! Did you mean this?\nnpm ERR!     npm run test # run the \"test\" package script"
            ),
            "npm run test"
        );
        assert_eq!(
            command(
                "gh isue list",
                1,
                "unknown command \"isue\" for \"gh\"\n\nDid you mean this?\n\tissue\n\nUsage:  gh <command> <subcommand> [flags]"
            ),
            "gh issue list"
        );
        assert_eq!(
            command(
                "pip instal requests",
                1,
                "ERROR: unknown command \"instal\" - maybe you meant \"install\""
            ),
            "pip install requests"
        );
        assert_eq!(
            command(
                "yarn instal",
                1,
                "error Command \"instal\" not found. Did you mean \"install\"?"
            ),
            "yarn install"
        );
        assert_eq!(
            command(
                "gem isntall rails",
                1,
                "ERROR:  Unknown command isntall\nDid you mean?  \"install\""
            ),
            "gem install rails"
        );
        assert_eq!(
            command(
                "terraform aply",
                1,
                "Terraform has no command named \"aply\". Did you mean \"apply\"?"
            ),
            "terraform apply"
        );
        assert_eq!(
            command(
                "hg lgo",
                255,
                "hg: unknown command 'lgo'\n(did you mean log?)"
            ),
            "hg log"
        );
        assert_eq!(
            command(
                "hg lo",
                255,
                "hg: unknown command 'lo'\n(did you mean one of log, locate?)"
            ),
            "hg log"
        );
    }

    #[test]
    fn a_compilers_did_you_mean_about_a_variable_is_left_alone() {
        let out = "error[E0425]: cannot find value `cont` in this scope\n  --> src/main.rs:3:5\n   |\n3  |     cont += 1;\n   |     ^^^^ help: a local variable with a similar name exists: `count`";
        assert!(none("cargo build", 101, out));
    }

    #[test]
    fn pip_refused_for_the_system_installs_for_the_user() {
        let out = "ERROR: Could not install packages due to an OSError: [Errno 13] Permission denied: '/usr/lib/python3/dist-packages'";
        assert_eq!(
            command("pip3 install requests", 1, out),
            "pip3 install --user requests"
        );
        assert_eq!(
            command("pip3 install --user requests", 1, out),
            "sudo pip3 install --user requests",
            "refused even for the user: root, then"
        );
    }

    #[test]
    fn a_missing_python_module_is_installed_with_its_pypi_name() {
        let out = "Traceback (most recent call last):\n  File \"app.py\", line 1, in <module>\n    import yaml\nModuleNotFoundError: No module named 'yaml'";
        assert_eq!(
            command("python3 app.py", 1, out),
            "python3 -m pip install pyyaml && python3 app.py"
        );
        let out = "ModuleNotFoundError: No module named 'requests.adapters'";
        assert_eq!(command("pytest", 1, out), "pip install requests && pytest");
    }

    #[test]
    fn mkdir_cp_mv_and_touch_into_a_missing_directory_create_it_first() {
        assert_eq!(
            command("mkdir a/b/c", 1, "mkdir: a/b: No such file or directory"),
            "mkdir -p a/b/c"
        );
        assert_eq!(
            command(
                "mv x.txt out/2026/x.txt",
                1,
                "mv: rename x.txt to out/2026/x.txt: No such file or directory"
            ),
            "mkdir -p out/2026 && mv x.txt out/2026/x.txt"
        );
        assert_eq!(
            command(
                "cp x.txt out/y.txt",
                1,
                "cp: cannot create regular file 'out/y.txt': No such file or directory"
            ),
            "mkdir -p out && cp x.txt out/y.txt"
        );
        assert_eq!(
            command(
                "touch logs/app.log",
                1,
                "touch: logs/app.log: No such file or directory"
            ),
            "mkdir -p logs && touch logs/app.log"
        );
        let bsd = "mv: rename missing.txt to out/x.txt: No such file or directory";
        assert!(
            suggest_fix_from_output(
                &outcome("mv missing.txt out/x.txt", 1),
                &here(&["x.txt"]),
                bsd
            )
            .is_none(),
            "the source is what is missing, the directory tells"
        );
        assert_eq!(
            fixed_with("mv x.txt out/x.txt", 1, bsd, &here(&["x.txt"]))
                .command()
                .as_str(),
            "mkdir -p out && mv x.txt out/x.txt"
        );
        assert!(
            none(
                "mv missing.txt out/x.txt",
                1,
                "mv: cannot stat 'missing.txt': No such file or directory"
            ),
            "GNU says which one is missing"
        );
    }

    #[test]
    fn rm_grep_and_cp_on_a_directory_get_their_recursive_flag() {
        let fix = fixed("rm build", 1, "rm: build: is a directory");
        assert_eq!(fix.command().as_str(), "rm -r build");
        assert!(matches!(fix.danger(), Danger::Destructive(_)));
        assert_eq!(
            command("grep TODO src", 2, "grep: src: Is a directory"),
            "grep -r TODO src"
        );
        assert_eq!(
            command(
                "cp src backup",
                1,
                "cp: -r not specified; omitting directory 'src'"
            ),
            "cp -r src backup"
        );
        assert!(none("cp -a src backup", 1, "cp: omitting directory 'src'"));
    }

    #[test]
    fn a_changed_host_key_is_forgotten_with_a_warning_that_says_why_not_to() {
        let out = "@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@\n@    WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!     @\n@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@\nIT IS POSSIBLE THAT SOMEONE IS DOING SOMETHING NASTY!\nOffending ECDSA key in /Users/me/.ssh/known_hosts:12\nHost key for build.example.com has changed and you have requested strict checking.\nHost key verification failed.";
        let fix = fixed("ssh deploy@build.example.com", 255, out);
        assert_eq!(
            fix.command().as_str(),
            "ssh-keygen -R build.example.com && ssh deploy@build.example.com"
        );
        assert!(fix.rationale().contains("interception"));
        assert!(
            matches!(fix.danger(), Danger::Destructive(_)),
            "never pre-typed"
        );
    }

    #[test]
    fn short_help_becomes_long_help_where_only_that_works() {
        assert_eq!(
            command(
                "mytool -h",
                1,
                "mytool: invalid option -- 'h'\nTry 'mytool --help' for more information."
            ),
            "mytool --help"
        );
    }

    #[test]
    fn a_refusal_for_lack_of_permission_gets_sudo_with_its_yellow_line() {
        let fix = fixed(
            "touch /etc/hosts.new",
            1,
            "touch: /etc/hosts.new: Permission denied",
        );
        assert_eq!(fix.command().as_str(), "sudo touch /etc/hosts.new");
        assert_eq!(fix.danger(), &Danger::NeedsPrivilege);
        assert!(!fix.is_ghostable());
        assert_eq!(
            command(
                "systemctl restart nginx",
                1,
                "Failed to restart nginx.service: Access denied\nSee system logs and 'systemctl status nginx.service' for details.\nAuthentication is required to manage system services or units."
            ),
            "sudo systemctl restart nginx"
        );
    }

    #[test]
    fn sudo_is_not_offered_where_it_would_be_wrong() {
        assert!(
            command(
                "git push",
                128,
                "git@github.com: Permission denied (publickey).\nfatal: Could not read from remote repository."
            )
            .starts_with("ssh-add"),
            "an ssh key problem, not a root one"
        );
        assert!(
            none("./deploy.sh", 126, "fish: Permission denied"),
            "126 is the execute bit, the instant rule has it"
        );
        assert!(none("cd /root", 1, "cd: permission denied: /root"));
        assert!(
            none("sudo touch /etc/x", 1, "touch: /etc/x: Permission denied"),
            "already sudo"
        );
    }

    #[test]
    fn a_program_that_refuses_root_loses_its_sudo() {
        assert_eq!(
            command(
                "sudo yay -S foo",
                1,
                "-> Avoid running yay as root/sudo.\nyou cannot perform this operation as root"
            ),
            "yay -S foo"
        );
    }
}

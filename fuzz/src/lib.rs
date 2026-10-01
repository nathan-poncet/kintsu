//! The kintsu sources the fuzz targets reach, included by path. kintsu is
//! one binary with no library to link against, and the fuzzers only need
//! its pure rings: the entities, the use cases they lean on, and the one
//! controller that reads frames.
#![allow(dead_code, unused_imports)]

#[path = "../../src/entities/mod.rs"]
pub mod entities;

#[path = "../../src/use_cases/mod.rs"]
pub mod use_cases;

pub mod adapters;

use entities::{AliasFact, CommandLine, CommandOutcome, DirEntry, ExitStatus, Facts, Os};

/// A command line and a status cut from the fuzzer's bytes: the text up
/// to the first NUL, the byte after it as the status, the rest returned.
pub fn outcome_and_rest(data: &[u8]) -> Option<(CommandOutcome, &[u8])> {
    let split = data.iter().position(|b| *b == 0)?;
    let command = String::from_utf8_lossy(&data[..split]);
    let command = CommandLine::new(command.as_ref()).ok()?;
    let status = *data.get(split + 1)? as i32;
    let rest = data.get(split + 2..).unwrap_or(&[]);
    Some((CommandOutcome::new(command, ExitStatus::new(status)), rest))
}

/// A machine the rules can lean on: a few programs, a few entries here.
pub fn facts() -> Facts {
    Facts {
        os: Some(Os::Linux),
        executables: [
            "git", "python3", "pip", "pip3", "npm", "cargo", "sudo", "ls", "brew",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
        cwd_entries: vec![
            DirEntry {
                name: "src".into(),
                is_dir: true,
                is_executable: true,
            },
            DirEntry {
                name: "app.py".into(),
                is_dir: false,
                is_executable: false,
            },
            DirEntry {
                name: "deploy.sh".into(),
                is_dir: false,
                is_executable: false,
            },
        ],
        docker_desktop: false,
        aliases: vec![AliasFact {
            name: "hmz".into(),
            target: "~/.dotnet/tools/hmz".into(),
            target_found: false,
        }],
    }
}

/// What the property tests also state: a word the rules put in a fix is
/// one the user typed, a flag, the `&&` between two commands, or a plain
/// name; a line break appears only when the user typed one.
pub fn assert_nothing_from_the_output_is_syntax(typed: &CommandLine, proposed: &CommandLine) {
    let text = proposed.as_str();
    assert!(
        !text.contains(['\n', '\r']) || typed.as_str().contains(['\n', '\r']),
        "a line break in `{text}`"
    );
    let typed_words = typed.words();
    for word in proposed.words() {
        if typed_words.contains(&word) || word == "&&" || word.starts_with('-') {
            continue;
        }
        assert!(
            word.chars()
                .all(|c| c.is_alphanumeric() || "._-/@:+~=,".contains(c)),
            "`{word}` came from the output into `{text}`"
        );
    }
}

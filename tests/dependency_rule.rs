//! The entities under `src/entities` and the use cases under `src/use_cases`
//! may not know about the adapters, the terminal, processes, files, the
//! network, a runtime or the clock. This is the Dependency Rule, enforced.

use std::path::{Path, PathBuf};

/// Names that mean I/O, a runtime, a terminal or a clock.
const FORBIDDEN_IN_INNER_RINGS: &[&str] = &[
    "crate::adapters",
    "std::io",
    "std::fs",
    "std::process",
    "std::net",
    "std::os",
    "std::env",
    "std::thread",
    "std::time::Instant",
    "std::time::SystemTime",
    "tokio",
    "async_std",
    "crossterm",
    "ratatui",
    "reqwest",
    "rusqlite",
    "serde_json",
    "println!",
    "eprintln!",
    "print!",
    "eprint!",
    "dbg!",
];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("readable directory") {
        let path = entry.expect("directory entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

fn violations_under(ring: &str, extra_forbidden: &[&str]) -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(ring);
    let mut files = Vec::new();
    rust_files(&root, &mut files);
    assert!(!files.is_empty(), "no files found under {ring}");
    let mut violations = Vec::new();
    for file in files {
        let source = std::fs::read_to_string(&file).expect("readable source");
        for (number, line) in source.lines().enumerate() {
            for forbidden in FORBIDDEN_IN_INNER_RINGS.iter().chain(extra_forbidden) {
                if line.contains(forbidden) {
                    violations.push(format!(
                        "{}:{}: uses `{forbidden}`",
                        file.display(),
                        number + 1
                    ));
                }
            }
        }
    }
    violations
}

#[test]
fn the_entities_depend_on_nothing_outside_themselves() {
    let violations = violations_under("src/entities", &["crate::use_cases"]);
    assert!(
        violations.is_empty(),
        "an entity reaches outward:\n{}",
        violations.join("\n")
    );
}

#[test]
fn the_use_cases_depend_only_on_the_entities_and_their_ports() {
    let violations = violations_under("src/use_cases", &[]);
    assert!(
        violations.is_empty(),
        "a use case reaches outward:\n{}",
        violations.join("\n")
    );
}

#[test]
fn the_rings_exist_where_the_architecture_says() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for ring in [
        "entities",
        "use_cases",
        "use_cases/ports",
        "adapters/controllers",
        "adapters/presenters",
        "adapters/gateways",
    ] {
        assert!(
            src.join(ring).join("mod.rs").is_file(),
            "missing ring {ring}"
        );
    }
}

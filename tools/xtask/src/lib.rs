//! Guards of the architecture. `cargo xtask check` runs them; the tests in
//! `tests/dependency_rule.rs` run them under `cargo test`.

#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};

use cargo_metadata::{DependencyKind, Metadata};

/// The entities crate: a leaf.
pub const ENTITIES: &str = "kintsu-entities";
/// The use cases crate: entities only.
pub const USE_CASES: &str = "kintsu-use-cases";
/// The adapters crate: entities and use cases.
pub const ADAPTERS: &str = "kintsu-adapters";
/// The binary: a sink, nothing depends on it.
pub const APP: &str = "kintsu";
/// This crate: a sink that knows none of the others.
pub const XTASK: &str = "xtask";

/// A workspace crate and the workspace crates it depends on (normal
/// dependencies only: a fake in a dev-dependency does not leak into a build).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrateNode {
    /// The package name.
    pub name: String,
    /// The workspace packages it depends on.
    pub deps: Vec<String>,
    /// The directory holding its `Cargo.toml`.
    pub root: PathBuf,
}

/// Reads the workspace graph out of `cargo metadata --no-deps`.
pub fn workspace_from_metadata(metadata: &Metadata) -> Vec<CrateNode> {
    let members = metadata.workspace_packages();
    let names: Vec<&str> = members.iter().map(|p| p.name.as_str()).collect();
    members
        .iter()
        .map(|package| CrateNode {
            name: package.name.clone(),
            deps: package
                .dependencies
                .iter()
                .filter(|d| d.kind == DependencyKind::Normal && names.contains(&d.name.as_str()))
                .map(|d| d.name.clone())
                .collect(),
            root: package
                .manifest_path
                .parent()
                .map(|p| p.as_std_path().to_path_buf())
                .unwrap_or_default(),
        })
        .collect()
}

/// The crates of ours each ring may depend on. A crate absent from this
/// table is a violation in itself: adding a crate is a deliberate act.
fn allowed(name: &str) -> Option<&'static [&'static str]> {
    match name {
        ENTITIES => Some(&[]),
        USE_CASES => Some(&[ENTITIES]),
        ADAPTERS => Some(&[ENTITIES, USE_CASES]),
        APP => Some(&[ENTITIES, USE_CASES, ADAPTERS]),
        XTASK => Some(&[]),
        _ => None,
    }
}

/// Every crate depends only on crates of ours in its ring or deeper. The
/// sinks (`kintsu`, `xtask`) appear in nobody's allowlist, so nothing can
/// depend on them.
pub fn check_dependency_rule(crates: &[CrateNode]) -> Result<(), Vec<String>> {
    let mut violations = Vec::new();
    for node in crates {
        let Some(allowed) = allowed(&node.name) else {
            violations.push(format!(
                "`{}` is not a known ring; add it to `allowed` in tools/xtask",
                node.name
            ));
            continue;
        };
        for dep in &node.deps {
            if !allowed.contains(&dep.as_str()) {
                violations.push(format!(
                    "`{}` depends on `{dep}`, which its ring forbids",
                    node.name
                ));
            }
        }
    }
    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    }
}

/// Names that mean I/O, a runtime, a terminal or a clock. None may appear
/// in the sources of the entities or the use cases.
pub const FORBIDDEN_IN_INNER_RINGS: &[&str] = &[
    "std::io",
    "std::fs",
    "std::process",
    "std::net",
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
    "println!",
    "eprintln!",
    "print!",
    "eprint!",
    "dbg!",
];

/// The inner rings read no file, open no socket, spawn no process, print
/// nothing and never ask what time it is.
pub fn check_inner_rings_are_pure(crates: &[CrateNode]) -> Result<(), Vec<String>> {
    let mut violations = Vec::new();
    for node in crates
        .iter()
        .filter(|n| n.name == ENTITIES || n.name == USE_CASES)
    {
        for file in rust_files(&node.root.join("src")) {
            let source = fs::read_to_string(&file).unwrap_or_default();
            for (index, line) in source.lines().enumerate() {
                for token in FORBIDDEN_IN_INNER_RINGS {
                    if line.contains(token) {
                        violations.push(format!(
                            "{}:{}: `{token}` has no place in an inner ring",
                            file.display(),
                            index + 1
                        ));
                    }
                }
            }
        }
    }
    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    }
}

fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(rust_files(&path));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
    files.sort();
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(name: &str, deps: &[&str]) -> CrateNode {
        CrateNode {
            name: name.to_string(),
            deps: deps.iter().map(|d| d.to_string()).collect(),
            root: PathBuf::new(),
        }
    }

    #[test]
    fn a_use_case_reaching_an_adapter_is_reported() {
        let crates = [node(ENTITIES, &[]), node(USE_CASES, &[ENTITIES, ADAPTERS])];
        let violations = check_dependency_rule(&crates).unwrap_err();
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains(ADAPTERS), "{violations:?}");
    }

    #[test]
    fn an_unknown_crate_is_a_violation_until_it_is_given_a_ring() {
        let crates = [node("kintsu-mystery", &[])];
        assert!(check_dependency_rule(&crates).is_err());
    }

    #[test]
    fn the_intended_graph_passes() {
        let crates = [
            node(ENTITIES, &[]),
            node(USE_CASES, &[ENTITIES]),
            node(ADAPTERS, &[ENTITIES, USE_CASES]),
            node(APP, &[USE_CASES, ADAPTERS]),
            node(XTASK, &[]),
        ];
        assert_eq!(check_dependency_rule(&crates), Ok(()));
    }
}

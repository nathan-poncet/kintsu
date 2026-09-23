//! `cargo xtask check`: fail when the architecture is violated.

#![forbid(unsafe_code)]

use std::process::ExitCode;

use cargo_metadata::MetadataCommand;
use xtask::{check_dependency_rule, check_inner_rings_are_pure, workspace_from_metadata};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("check") => check(),
        _ => {
            eprintln!("usage: cargo xtask check");
            ExitCode::from(2)
        }
    }
}

fn check() -> ExitCode {
    let metadata = match MetadataCommand::new().no_deps().exec() {
        Ok(metadata) => metadata,
        Err(error) => {
            eprintln!("cargo metadata failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    let crates = workspace_from_metadata(&metadata);
    let checks = [
        ("dependency rule", check_dependency_rule(&crates)),
        ("inner rings are pure", check_inner_rings_are_pure(&crates)),
    ];
    let mut failed = false;
    for (name, result) in checks {
        match result {
            Ok(()) => println!("ok    {name}"),
            Err(violations) => {
                failed = true;
                println!("FAIL  {name}");
                for violation in violations {
                    println!("      {violation}");
                }
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

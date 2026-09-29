//! `kintsu learned`: the fixes taken twice, listed or forgotten.

use std::io::Write;
use std::process::ExitCode;

use crate::adapters::controllers::LearnedAction;
use crate::adapters::gateways::JsonLearnedFixes;
use crate::adapters::presenters::{Style, forgotten, learned_json, learned_report};
use crate::use_cases::{Forget, Learned};

use super::failure;

pub(super) fn run(
    store: &JsonLearnedFixes,
    action: LearnedAction,
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> ExitCode {
    let learned = Learned { learned: store };
    match action {
        LearnedAction::List { json } => match learned.entries() {
            Ok(entries) => {
                let text = if json {
                    learned_json(&entries)
                } else {
                    learned_report(&entries, style)
                };
                let _ = writeln!(out, "{text}");
                ExitCode::SUCCESS
            }
            Err(e) => failure(err, &e.to_string(), style, json),
        },
        LearnedAction::Forget { program } => {
            let what = match &program {
                Some(program) => Forget::Program(program.clone()),
                None => Forget::All,
            };
            match learned.forget(&what) {
                Ok(count) => {
                    let _ = writeln!(out, "{}", forgotten(count, program.as_deref(), style));
                    ExitCode::SUCCESS
                }
                Err(e) => failure(err, &e.to_string(), style, false),
            }
        }
    }
}

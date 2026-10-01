#![no_main]
//! The instant rules, on the quiet path: any command line and status, with
//! a few programs and entries to lean on, never panic.

use kintsu_fuzz::entities::{suggest_explanation, suggest_fix};
use kintsu_fuzz::{facts, outcome_and_rest};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((outcome, _)) = outcome_and_rest(data) else {
        return;
    };
    if let Some(fix) = suggest_fix(&outcome, &facts()) {
        assert!(
            !fix.command().as_str().contains('\n') || outcome.command().as_str().contains('\n'),
            "a line break the user did not type"
        );
    }
    let _ = suggest_explanation(&outcome, &facts());
});

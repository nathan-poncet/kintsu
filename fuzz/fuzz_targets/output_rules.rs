#![no_main]
//! The rules that read a command's output: any bytes as the command, the
//! status and the output, no panic, and nothing from the output becomes
//! shell syntax in the fix.

use kintsu_fuzz::entities::suggest_fix_from_output;
use kintsu_fuzz::{assert_nothing_from_the_output_is_syntax, facts, outcome_and_rest};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((outcome, rest)) = outcome_and_rest(data) else {
        return;
    };
    let output = String::from_utf8_lossy(rest);
    if let Some(fix) = suggest_fix_from_output(&outcome, &facts(), &output) {
        assert_nothing_from_the_output_is_syntax(outcome.command(), fix.command());
    }
});

#![no_main]
//! Cutting a command's output from a screen dump: any screen, any command
//! line, any line budget.

use kintsu_fuzz::entities::output_after;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some(split) = data.iter().position(|b| *b == 0) else {
        return;
    };
    let command = String::from_utf8_lossy(&data[..split]);
    let max_lines = usize::from(data.get(split + 1).copied().unwrap_or(40));
    let screen = String::from_utf8_lossy(data.get(split + 2..).unwrap_or(&[]));
    if let Some(output) = output_after(&screen, &command, max_lines) {
        assert!(output.lines().count() <= max_lines.max(1));
    }
});

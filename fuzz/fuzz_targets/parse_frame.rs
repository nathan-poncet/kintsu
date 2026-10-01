#![no_main]
//! The daemon's frame parser: any line a local client could send is a
//! frame or an error, never a panic.

use kintsu_fuzz::adapters::controllers::socket::parse_frame;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let line = String::from_utf8_lossy(data);
    let _ = parse_frame(&line);
});

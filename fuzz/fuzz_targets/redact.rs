#![no_main]
//! Redaction: any text, no panic; a text with nothing to find comes back
//! byte for byte.

use kintsu_fuzz::entities::redact;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let redacted = redact(&text);
    if redacted.findings().is_empty() {
        assert_eq!(redacted.text(), text.as_ref());
    }
});

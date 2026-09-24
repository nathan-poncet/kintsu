//! Outbound adapters: from what a use case produced to what a surface shows.
//! Pure functions of a value and a style; they never read the terminal.

pub mod doctor;
pub mod frames;
pub mod plain;
pub mod shell_hook;
pub mod style;
pub mod toast;

pub use doctor::doctor_report;
pub use plain::{
    error_line, explanation, fix_report, hand_off_notice, ignored, privacy_report, raw_fix,
};
pub use shell_hook::shell_hook;
pub use style::Style;
pub use toast::{message_toast, toast};

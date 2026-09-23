//! Outbound adapters: from what a use case produced to what a surface shows.

pub mod hint;
pub mod shell_hook;

pub use hint::hint;
pub use shell_hook::shell_hook;

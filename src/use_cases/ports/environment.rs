//! What the rules may look at on the machine.

use crate::entities::{DirEntry, Os};

/// Reads the machine: which system, which programs, which files.
pub trait Environment {
    /// The operating system family.
    fn os(&self) -> Option<Os>;
    /// Every program name reachable from the shell: PATH, builtins, aliases.
    fn executables(&self) -> Vec<String>;
    /// The entries of a directory; empty when it cannot be read.
    fn entries(&self, dir: &str) -> Vec<DirEntry>;
}

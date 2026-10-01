//! What the rules may look at on the machine.

use crate::entities::{AliasFact, DirEntry, Os};

/// Reads the machine: which system, which programs, which files.
pub trait Environment {
    /// The operating system family.
    fn os(&self) -> Option<Os>;
    /// Every program name reachable from the shell: PATH, builtins, and
    /// the shell's aliases and functions when it said what they are.
    fn executables(&self) -> Vec<String>;
    /// The entries of a directory; empty when it cannot be read.
    fn entries(&self, dir: &str) -> Vec<DirEntry>;
    /// Whether Docker Desktop is installed: `/Applications/Docker.app`.
    fn docker_desktop(&self) -> bool;

    /// The shell's aliases with their programs looked up; empty when the
    /// shell said nothing about them.
    fn aliases(&self) -> Vec<AliasFact> {
        Vec::new()
    }
}

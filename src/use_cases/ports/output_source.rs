//! Reading what a terminal or a multiplexer remembers of a pane.

use crate::entities::TerminalIdentity;

/// Reads the recent text of the pane a shell runs in.
pub trait OutputSource {
    /// The last `lines` lines the pane shows or remembers, oldest first,
    /// or nothing when no source applies to this pane or it failed.
    fn recent(&self, terminal: &TerminalIdentity, lines: usize) -> Option<String>;
}

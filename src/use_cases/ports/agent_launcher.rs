//! Starting a command-line agent in the user's terminal, with a brief.

use thiserror::Error;

use crate::entities::ModelSpec;

/// Why an agent did not start or did not finish.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AgentError {
    /// The command is not on the PATH.
    #[error("`{0}` is not installed")]
    NotInstalled(String),
    /// It started and ended badly.
    #[error("agent failed: {0}")]
    Failed(String),
}

/// Launches agents and waits for them.
pub trait AgentLauncher {
    /// Runs the agent with the brief, in the foreground, until it exits.
    fn launch(&self, spec: &ModelSpec, brief: &str) -> Result<(), AgentError>;
}

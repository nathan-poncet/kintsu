//! Launching a CLI agent: the brief goes to a private file, the template
//! runs through `sh` with the terminal, and the file is removed after.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::entities::ModelSpec;
use crate::use_cases::ports::{AgentError, AgentLauncher};

pub struct ShellAgents {
    brief_dir: PathBuf,
}

impl ShellAgents {
    /// Briefs are written under this directory, one file per launch.
    pub fn new(brief_dir: impl Into<PathBuf>) -> Self {
        Self {
            brief_dir: brief_dir.into(),
        }
    }
}

/// The shell line to run: `{brief}` becomes the file path; a template
/// without it gets the brief's content as its last argument.
pub fn shell_line(template: &str, brief_path: &Path) -> String {
    let path = brief_path.display().to_string();
    if template.contains("{brief}") {
        template.replace("{brief}", &path)
    } else {
        format!("{template} \"$(cat {path})\"")
    }
}

impl AgentLauncher for ShellAgents {
    fn launch(&self, spec: &ModelSpec, brief: &str) -> Result<(), AgentError> {
        std::fs::create_dir_all(&self.brief_dir)
            .map_err(|e| AgentError::Failed(format!("{}: {e}", self.brief_dir.display())))?;
        let path = self
            .brief_dir
            .join(format!("brief-{}.md", std::process::id()));
        write_private(&path, brief)
            .map_err(|e| AgentError::Failed(format!("{}: {e}", path.display())))?;
        let line = shell_line(&spec.model, &path);
        let status = Command::new("sh").arg("-c").arg(&line).status();
        let _ = std::fs::remove_file(&path);
        let status = status.map_err(|e| AgentError::Failed(e.to_string()))?;
        match status.code() {
            Some(127) => Err(AgentError::NotInstalled(
                spec.model
                    .split_whitespace()
                    .next()
                    .unwrap_or_default()
                    .to_string(),
            )),
            _ => Ok(()),
        }
    }
}

#[cfg(unix)]
fn write_private(path: &Path, content: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(content.as_bytes())
}

#[cfg(not(unix))]
fn write_private(path: &Path, content: &str) -> std::io::Result<()> {
    std::fs::write(path, content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{KeySource, Provider, Tier};

    fn agent(template: &str) -> ModelSpec {
        ModelSpec {
            name: "a".into(),
            provider: Provider::CliAgent,
            model: template.into(),
            base_url: None,
            key: KeySource::None,
            tier: Tier::Agent,
            timeout: None,
            max_output_tokens: None,
        }
    }

    #[test]
    fn the_placeholder_is_the_file_and_its_absence_means_the_content() {
        let p = Path::new("/tmp/b.md");
        assert_eq!(
            shell_line("aider --message-file {brief}", p),
            "aider --message-file /tmp/b.md"
        );
        assert_eq!(
            shell_line("claude \"$(cat {brief})\"", p),
            "claude \"$(cat /tmp/b.md)\""
        );
        assert_eq!(shell_line("myagent", p), "myagent \"$(cat /tmp/b.md)\"");
    }

    #[test]
    fn the_brief_reaches_the_agent_and_the_file_is_removed_after() {
        let dir = std::env::temp_dir().join(format!("kintsu-agents-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let out = dir.join("seen.txt");
        let launcher = ShellAgents::new(&dir);
        let template = format!("cat {{brief}} > {}", out.display());
        launcher
            .launch(&agent(&template), "# brief\nhello")
            .unwrap();
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "# brief\nhello");
        assert!(
            !std::fs::read_dir(&dir)
                .unwrap()
                .flatten()
                .any(|e| e.file_name().to_string_lossy().starts_with("brief-"))
        );
        let missing = launcher.launch(&agent("kintsu-no-such-agent-xyz {brief}"), "b");
        assert_eq!(
            missing,
            Err(AgentError::NotInstalled("kintsu-no-such-agent-xyz".into()))
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}

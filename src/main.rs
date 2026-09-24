//! kintsu: when a command fails, fix it, understand it, or hand it to an
//! AI agent, in the user's own terminal and shell. `main` reads the
//! environment once and hands over to the composition root in `app`.

#![forbid(unsafe_code)]

mod adapters;
mod app;
mod entities;
mod use_cases;

use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;

use entities::SessionId;

fn main() -> ExitCode {
    let home = std::env::var("HOME").ok().filter(|h| !h.is_empty());
    let dir_from = |explicit: &str, xdg: &str, fallback: &str, tail: &str| -> PathBuf {
        if let Some(p) = std::env::var_os(explicit).filter(|p| !p.is_empty()) {
            return PathBuf::from(p);
        }
        let base = std::env::var_os(xdg)
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(home.clone().unwrap_or_else(|| ".".into())).join(fallback)
            });
        base.join("kintsu").join(tail)
    };
    let runtime = app::Runtime {
        args: std::env::args().skip(1).collect(),
        session: std::env::var("KINTSU_SESSION")
            .ok()
            .filter(|s| !s.is_empty())
            .map(SessionId::new),
        cwd: std::env::current_dir()
            .ok()
            .map(|p| p.display().to_string()),
        config_path: dir_from("KINTSU_CONFIG", "XDG_CONFIG_HOME", ".config", "config.toml"),
        state_dir: dir_from("KINTSU_STATE_DIR", "XDG_STATE_HOME", ".local/state", ""),
        home,
        path_var: std::env::var("PATH").unwrap_or_default(),
        color: std::env::var_os("NO_COLOR").is_none()
            && std::io::stderr().is_terminal()
            && std::io::stdout().is_terminal(),
        debug: std::env::var_os("KINTSU_DEBUG").is_some(),
    };
    let (stdout, stderr) = (std::io::stdout(), std::io::stderr());
    app::run(&runtime, &mut stdout.lock(), &mut stderr.lock())
}

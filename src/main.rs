//! kintsu: when a command fails, fix it, understand it, or hand it to an
//! AI agent, in the user's own terminal and shell. `main` reads the
//! environment once and hands over to the composition root in `app`.

#![deny(unsafe_code)]

mod adapters;
mod app;
mod daemon;
mod entities;
mod use_cases;

use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;

use entities::{SessionId, TerminalIdentity};

fn main() -> ExitCode {
    let home = std::env::var("HOME").ok().filter(|h| !h.is_empty());
    let var_path = |name: &str| {
        std::env::var_os(name)
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
    };
    let under_home =
        |fallback: &str| PathBuf::from(home.clone().unwrap_or_else(|| ".".into())).join(fallback);
    let config_path = var_path("KINTSU_CONFIG").unwrap_or_else(|| {
        var_path("XDG_CONFIG_HOME")
            .unwrap_or_else(|| under_home(".config"))
            .join("kintsu")
            .join("config.toml")
    });
    let state_dir = var_path("KINTSU_STATE_DIR").unwrap_or_else(|| {
        var_path("XDG_STATE_HOME")
            .unwrap_or_else(|| under_home(".local/state"))
            .join("kintsu")
    });
    let socket_path = var_path("KINTSU_SOCKET").unwrap_or_else(|| {
        var_path("XDG_RUNTIME_DIR")
            .map(|d| d.join("kintsu").join("daemon.sock"))
            .unwrap_or_else(|| state_dir.join("daemon.sock"))
    });
    let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
    let terminal = TerminalIdentity {
        program: var("TERM_PROGRAM"),
        tmux_pane: var("TMUX_PANE"),
        tmux_socket: var("TMUX").and_then(|t| t.split(',').next().map(String::from)),
        herdr_pane: var("HERDR_PANE_ID"),
        herdr_socket: var("HERDR_SOCKET_PATH"),
        herdr_bin: var("HERDR_BIN_PATH"),
        wezterm_pane: var("WEZTERM_PANE"),
        kitty_window: var("KITTY_WINDOW_ID"),
        kitty_listen_on: var("KITTY_LISTEN_ON"),
        iterm_session: var("ITERM_SESSION_ID"),
    };
    let runtime = app::Runtime {
        args: std::env::args().skip(1).collect(),
        terminal,
        session: std::env::var("KINTSU_SESSION")
            .ok()
            .filter(|s| !s.is_empty())
            .map(SessionId::new),
        cwd: std::env::current_dir()
            .ok()
            .map(|p| p.display().to_string()),
        log_path: state_dir.join("daemon.log"),
        exe: std::env::current_exe().unwrap_or_else(|_| PathBuf::from("kintsu")),
        config_path,
        state_dir,
        socket_path,
        home,
        path_var: std::env::var("PATH").unwrap_or_default(),
        color: std::env::var_os("NO_COLOR").is_none()
            && std::io::stderr().is_terminal()
            && std::io::stdout().is_terminal(),
        tty_color: std::env::var_os("NO_COLOR").is_none() && std::io::stderr().is_terminal(),
        terminal_color: std::env::var_os("NO_COLOR").is_none()
            && std::fs::File::open("/dev/tty").is_ok(),
        debug: std::env::var_os("KINTSU_DEBUG").is_some(),
        daemon: std::env::var_os("KINTSU_NO_DAEMON").is_none(),
    };
    // Unlocked handles: the daemon's threads log from outside `main`, and a
    // lock held here for the whole run would block them.
    app::run(&runtime, &mut std::io::stdout(), &mut std::io::stderr())
}

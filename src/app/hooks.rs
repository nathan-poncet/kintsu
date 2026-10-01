//! What the hooks call: `kintsu session new` once at the shell's start,
//! `kintsu triage` after every command line, and `kintsu subscribe`, the
//! zsh child that waits for messages.

use std::collections::BTreeMap;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::process::ExitCode;

use crate::adapters::gateways::{
    DaemonClient, EnvSecrets, HookNotes, RandomIds, SystemClock, TerminalOutput, unix,
};
use crate::adapters::presenters::{error_line, toast};
use crate::entities::{KeySource, SessionDetails, SessionId, Settings, Shell, TriageDecision};
use crate::use_cases::ports::Secrets;
use crate::use_cases::{CaptureOutput, Triage, TriageInput};

use super::{Local, Runtime};

/// The hook's call: the daemon within the budget when it may, the local
/// path otherwise. The prompt never waits longer than the budget.
pub(super) fn triage(
    rt: &Runtime,
    local: &Local<'_>,
    mut input: TriageInput,
    signal_pid: Option<u32>,
    err: &mut dyn Write,
) -> ExitCode {
    let Local {
        settings,
        state,
        environment,
        style,
        learned,
    } = *local;
    input.terminal = rt.terminal.clone();
    input.path = Some(rt.path_var.clone());
    input.env = keys_the_models_read(settings);
    input.language = rt.machine_language();
    if let Some(session) = &input.session {
        input.terminal.stderr_copy = HookNotes::new(&rt.state_dir).stderr_copy(session);
    }
    let budget = sync_budget(settings);
    if rt.daemon
        && let Some(view) = rt
            .client()
            .command_finished(&input, rt.color, signal_pid, budget)
    {
        for text in view.toast.iter().chain(view.bubbles.iter()) {
            let _ = writeln!(err, "{text}");
        }
        if let Some(session) = &input.session {
            let notes = HookNotes::new(&rt.state_dir);
            // The "asking…" line, the toast's last when a model is asked,
            // is transient: the hook accounts for it while it waits.
            let toast_rows = view.toast.as_deref().map(|toast| match view.pending {
                Some(_) => toast.rsplit_once('\n').map_or("", |(kept, _)| kept),
                None => toast,
            });
            let bubble: Vec<&str> = toast_rows
                .into_iter()
                .filter(|t| !t.is_empty())
                .chain(view.bubbles.iter().map(String::as_str))
                .collect();
            if !bubble.is_empty() {
                let _ = notes.set_bubble(session, &bubble.join("\n"));
            }
            if view.pending.is_some() {
                let _ = notes.set_asking(session);
            }
            if let Some(ghost) = &view.ghost {
                let _ = notes.set_ghost(session, ghost);
            }
        }
        return ExitCode::SUCCESS;
    }
    let triage = Triage {
        settings,
        clock: &SystemClock,
        ids: &RandomIds,
        sessions: state,
        cases: state,
        ignores: state,
        environment,
        learned,
    };
    let ghost_shell = input.shell.is_some_and(Shell::supports_ghost_text);
    let session = input.session.clone();
    match triage.run(input) {
        Ok(decision) => {
            if let Some(text) = toast(&decision, style, ghost_shell) {
                let _ = writeln!(err, "{text}");
                if let Some(session) = &session {
                    let _ = HookNotes::new(&rt.state_dir).set_bubble(session, &text);
                }
            }
            if let (TriageDecision::Offer { fix: Some(fix), .. }, Some(session), true) =
                (&decision, &session, ghost_shell)
                && fix.is_ghostable()
            {
                let _ = HookNotes::new(&rt.state_dir).set_ghost(session, fix.command().as_str());
            }
            // The output is read after the bubble: it costs a program run,
            // and only an offered case is worth it.
            if let TriageDecision::Offer { case, .. } = decision {
                let capture = CaptureOutput {
                    settings,
                    output: &TerminalOutput::new(settings.capture.sources.clone()),
                    cases: state,
                };
                let _ = capture.run(*case, &rt.terminal);
            }
        }
        Err(e) if rt.debug => {
            let _ = writeln!(err, "{}", error_line(&e.to_string(), style));
        }
        Err(_) => {}
    }
    ExitCode::SUCCESS
}

/// The values of the variables the configured models read their keys
/// from, as this shell sees them; nothing else of the environment. The
/// daemon, under launchd or systemd, has none of them.
fn keys_the_models_read(settings: &Settings) -> BTreeMap<String, String> {
    settings
        .models
        .iter()
        .filter_map(|model| match &model.key {
            KeySource::Env(var) => EnvSecrets
                .lookup(&model.key)
                .map(|value| (var.clone(), value)),
            _ => None,
        })
        .collect()
}

/// `kintsu session new`: what the hook says once, at the shell's start, so
/// the daemon knows the shell before its first failure. Nothing waits on
/// it: a daemon that is not there is started for the next frame, and the
/// shell's start never fails for it.
pub(super) fn session_new(
    rt: &Runtime,
    settings: &Settings,
    session: Option<SessionId>,
    shell: Option<Shell>,
    pid: Option<u32>,
    tty: Option<String>,
) -> ExitCode {
    let Some(session) = session.or_else(|| rt.session.clone()) else {
        return ExitCode::SUCCESS;
    };
    if !rt.daemon {
        return ExitCode::SUCCESS;
    }
    let details = SessionDetails {
        shell,
        pid,
        tty: tty.or_else(|| unix::tty_name(std::io::stdin().as_raw_fd())),
        terminal: rt.terminal.clone(),
        path: Some(rt.path_var.clone()),
        env: keys_the_models_read(settings),
        language: rt.machine_language(),
    };
    let _ = rt
        .client()
        .session_new(&session, &details, sync_budget(settings));
    ExitCode::SUCCESS
}

/// How long a hook waits for the daemon, as the configuration says.
fn sync_budget(settings: &Settings) -> std::time::Duration {
    std::time::Duration::from_millis(settings.daemon.sync_budget.as_millis())
}

/// `kintsu subscribe`: prints every bubble the daemon sends for the session
/// until the daemon or the parent shell goes away.
pub(super) fn subscribe(rt: &Runtime, session: Option<SessionId>, out: &mut dyn Write) -> ExitCode {
    let Some(session) = session.or_else(|| rt.session.clone()) else {
        return ExitCode::from(2);
    };
    if !rt.daemon {
        return ExitCode::from(1);
    }
    let Ok(stream) = rt.client().subscribe(&session, rt.terminal_color) else {
        return ExitCode::from(1);
    };
    let notes = HookNotes::new(&rt.state_dir);
    let _ = DaemonClient::follow(stream, |text| {
        let _ = writeln!(out, "{text}");
        let _ = out.flush();
        let _ = notes.append_bubble(&session, text);
    });
    ExitCode::SUCCESS
}

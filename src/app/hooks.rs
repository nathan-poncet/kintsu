//! What the hooks call: `kintsu triage` after every command line, and
//! `kintsu subscribe`, the zsh child that waits for messages.

use std::io::Write;
use std::process::ExitCode;

use crate::adapters::gateways::{DaemonClient, HookNotes, RandomIds, SystemClock, TerminalOutput};
use crate::adapters::presenters::{error_line, toast};
use crate::entities::{SessionId, Shell, TriageDecision};
use crate::use_cases::{CaptureOutput, Triage, TriageInput};

use super::{Local, Runtime, SYNC_BUDGET};

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
    } = *local;
    input.terminal = rt.terminal.clone();
    if rt.daemon
        && let Some(view) = rt
            .client()
            .command_finished(&input, rt.color, signal_pid, SYNC_BUDGET)
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

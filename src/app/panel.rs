//! `kintsu panel`: what `^K` runs. The panel is drawn on the tty; the
//! models are asked from a thread while it keeps drawing.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use crate::adapters::gateways::tty_panel;
use crate::adapters::gateways::{
    EnvSecrets, FsEnvironment, HookNotes, HttpModels, JsonState, SystemClock,
};
use crate::adapters::presenters::panel::{Arrival, Ask, Effect, Panel};
use crate::adapters::presenters::{Style, ignored, privacy_report};
use crate::entities::{Provider, SessionId, Settings, UiMode};
use crate::use_cases::ports::CaseStore;
use crate::use_cases::{Explain, FixLast, Focus, Ignore, Privacy};

use super::{Local, Runtime, failure};

/// `kintsu setup`: what the machine has, three questions, one file, and
/// `kintsu panel --above <rows>`: the last bubble expanded in its own
/// place, drawn on the tty; what the user takes comes out on stdout for
/// the hook to insert. `rows` lead from the cursor to the prompt's first
/// line; the bubble's own rows are known from the marker the printers
/// leave. Once the shell has moved past the failure, only the prompt is
/// cleared for the hook to redraw.
pub(super) fn expand(
    rt: &Runtime,
    local: &Local<'_>,
    session: Option<&SessionId>,
    above: u16,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> ExitCode {
    let Local {
        settings,
        state,
        environment,
        style,
    } = *local;
    let focus = Focus {
        sessions: state,
        cases: state,
    };
    let case = match focus.case(session) {
        Ok(Some(case)) => case,
        Ok(None) => {
            let _ = tty_panel::clear_above(above);
            return ExitCode::SUCCESS;
        }
        Err(e) => return failure(err, &e.to_string(), style, false),
    };
    let bubble = session.and_then(|s| HookNotes::new(&rt.state_dir).bubble(s).ok().flatten());
    let fix_last = FixLast {
        settings,
        cases: state,
        environment,
        secrets: &EnvSecrets,
        models: &HttpModels,
    };
    let known = fix_last.known(&case);
    let can_ask_fix = fix_last.candidate(&case).is_some();
    let agents: Vec<String> = settings
        .models
        .iter()
        .filter(|m| m.provider == Provider::CliAgent)
        .map(|m| m.name.clone())
        .collect();
    let default_agent = settings
        .routing
        .investigate
        .iter()
        .find(|name| agents.contains(name))
        .cloned();
    let privacy = match (Privacy { cases: state }).run(session) {
        Ok(doc) => without_seams(&privacy_report(&doc, &Style::BARE)),
        Err(e) => e.to_string(),
    };
    let mut panel = Panel::new(
        &case,
        known,
        can_ask_fix,
        agents,
        default_agent.as_deref(),
        privacy,
    );
    let panel_style = Style {
        color: rt.tty_color,
        ascii: settings.ui.ascii,
        mode: UiMode::Toast,
        links: false,
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let asker = Asker {
        settings: settings.clone(),
        state_dir: rt.state_dir.clone(),
        path_var: rt.path_var.clone(),
        session: session.cloned(),
    };
    let mut outside = |effect: Effect, panel: &mut Panel| match effect {
        Effect::Ask(ask) => asker.ask(ask, tx.clone()),
        Effect::Ignore(request) => {
            let ignore = Ignore {
                clock: &SystemClock,
                cases: state,
                ignores: state,
            };
            let text = match ignore.run(session, request) {
                Ok(entry) => without_seams(&ignored(&entry, &Style::BARE)),
                Err(e) => e.to_string(),
            };
            panel.receive(Arrival::Ignored(text));
        }
        Effect::Nothing | Effect::Insert(_) | Effect::Copy(_) | Effect::Close => {}
    };
    let first = panel.open();
    outside(first, &mut panel);
    match tty_panel::run(
        &mut panel,
        &panel_style,
        &rx,
        &mut outside,
        above,
        bubble.as_deref(),
    ) {
        Ok(Some(text)) => {
            let _ = writeln!(out, "{text}");
            ExitCode::SUCCESS
        }
        Ok(None) => ExitCode::SUCCESS,
        Err(e) => failure(err, &format!("panel: {e}"), style, false),
    }
}

/// What the panel's questions to models need, owned, so a thread can ask
/// while the panel keeps drawing.
struct Asker {
    settings: Settings,
    state_dir: PathBuf,
    path_var: String,
    session: Option<SessionId>,
}

impl Asker {
    fn ask(&self, ask: Ask, tx: std::sync::mpsc::Sender<Arrival>) {
        let settings = self.settings.clone();
        let state_dir = self.state_dir.clone();
        let path_var = self.path_var.clone();
        let session = self.session.clone();
        std::thread::spawn(move || {
            let state = JsonState::new(&state_dir);
            match ask {
                Ask::Explain => {
                    let explain = Explain {
                        settings: &settings,
                        cases: &state,
                        secrets: &EnvSecrets,
                        models: &HttpModels,
                    };
                    match explain.candidate(session.as_ref()) {
                        Ok(model) => {
                            let _ = tx.send(Arrival::Asking(Ask::Explain, model));
                        }
                        Err(e) => {
                            let _ = tx.send(Arrival::Explanation(Err(e.to_string())));
                            return;
                        }
                    }
                    let answer = explain
                        .run(session.as_ref())
                        .map(|e| (e.model, e.text))
                        .map_err(|e| e.to_string());
                    let _ = tx.send(Arrival::Explanation(answer));
                }
                Ask::Fix => {
                    let environment = FsEnvironment::new(path_var);
                    let fix_last = FixLast {
                        settings: &settings,
                        cases: &state,
                        environment: &environment,
                        secrets: &EnvSecrets,
                        models: &HttpModels,
                    };
                    if let Ok(Some(case)) = state.last(session.as_ref())
                        && let Some(model) = fix_last.candidate(&case)
                    {
                        let _ = tx.send(Arrival::Asking(Ask::Fix, model));
                    }
                    let answer = fix_last
                        .run(session.as_ref())
                        .map_err(|e| e.to_string())
                        .and_then(|proposal| {
                            if proposal.fix.is_none() && !proposal.failures.is_empty() {
                                let reasons: Vec<String> = proposal
                                    .failures
                                    .iter()
                                    .map(|(name, e)| format!("{name}: {e}"))
                                    .collect();
                                Err(format!("no model answered ({})", reasons.join("; ")))
                            } else {
                                Ok(proposal.fix)
                            }
                        });
                    let _ = tx.send(Arrival::Fix(answer));
                }
            }
        });
    }
}

/// Text meant for the panel: the seam is the panel's, not the line's.
fn without_seams(text: &str) -> String {
    text.lines()
        .map(|line| line.strip_prefix("| ").unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n")
}

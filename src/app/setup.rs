//! `kintsu setup`: three questions and the configuration is written.

use std::io::Write;
use std::process::ExitCode;

use crate::adapters::controllers::Prompter;
use crate::adapters::gateways::{EnvSecrets, FsEnvironment, HttpModels, render_settings};
use crate::adapters::presenters::doctor::Places;
use crate::adapters::presenters::{Style, doctor_report};
use crate::entities::UiMode;
use crate::use_cases::{Detect, Diagnose, compose};

use super::{Runtime, failure};

/// doctor's verdict on it.
pub(super) fn run(rt: &Runtime, yes: bool, out: &mut dyn Write, err: &mut dyn Write) -> ExitCode {
    let plain = Style {
        color: rt.color,
        ascii: false,
        mode: UiMode::Toast,
        links: false,
    };
    let environment = FsEnvironment::new(rt.path_var.clone());
    let detected = Detect {
        environment: &environment,
        models: &HttpModels,
    }
    .run();
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut prompter = Prompter {
        input: &mut input,
        output: out,
        assume_yes: yes,
    };
    if rt.config_path.is_file() && !yes {
        let _ = writeln!(
            prompter.output,
            "{}",
            plain.line(&format!("{} exists.", rt.config_path.display()))
        );
        let mut line = String::new();
        let _ = write!(prompter.output, "▎ Replace it? [y/N] ");
        let _ = prompter.output.flush();
        let _ = prompter.input.read_line(&mut line);
        if !matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            let _ = writeln!(prompter.output, "{}", plain.line("kept as is."));
            return ExitCode::SUCCESS;
        }
    }
    let answers = prompter.run(&detected);
    let settings = compose(&answers);
    if let Some(parent) = rt.config_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(&rt.config_path, render_settings(&settings)) {
        return failure(
            err,
            &format!("cannot write {}: {e}", rt.config_path.display()),
            &plain,
            false,
        );
    }
    let _ = writeln!(
        out,
        "{}",
        plain.line(&format!("written to {}", rt.config_path.display()))
    );
    let checks = Diagnose {
        settings: &settings,
        secrets: &EnvSecrets,
        environment: &environment,
        models: &HttpModels,
    }
    .run(rt.session.as_ref());
    let places = Places {
        config: &rt.config_path.display().to_string(),
        config_exists: true,
        state: &rt.state_dir.display().to_string(),
    };
    let _ = writeln!(
        out,
        "
{}",
        doctor_report(&checks, &places, &plain)
    );
    ExitCode::SUCCESS
}

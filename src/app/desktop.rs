//! What the desktop calls: `kintsu open` on a `kintsu://` click, and
//! `kintsu service` to register the daemon and the scheme.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use crate::adapters::controllers::{ServiceAction, parse_act_url};
use crate::adapters::gateways::service;
use crate::adapters::presenters::Style;

use super::{Runtime, failure};

/// `kintsu open kintsu://act?case=…&do=…`: a click, handed to the daemon,
/// which answers in the shell the case came from.
pub(super) fn open(rt: &Runtime, url: &str, err: &mut dyn Write, plain: &Style) -> ExitCode {
    let (case, action) = match parse_act_url(url) {
        Ok(parsed) => parsed,
        Err(e) => return failure(err, &e.to_string(), plain, false),
    };
    match rt.client().act(&case, action) {
        Some(Ok(())) => ExitCode::SUCCESS,
        Some(Err(reason)) => failure(err, &reason, plain, false),
        None => failure(
            err,
            "no daemon is running; open a hooked shell first",
            plain,
            false,
        ),
    }
}

/// `kintsu service install|uninstall`: launchd or systemd keeps the daemon
/// up, and the desktop hands `kintsu://` links to `kintsu open`.
pub(super) fn service(
    rt: &Runtime,
    action: ServiceAction,
    out: &mut dyn Write,
    err: &mut dyn Write,
    plain: &Style,
) -> ExitCode {
    let Some(home) = rt.home.clone() else {
        return failure(err, "HOME is not set", plain, false);
    };
    let paths = service::ServicePaths {
        home: PathBuf::from(home),
        state_dir: rt.state_dir.clone(),
        exe: rt.exe.clone(),
    };
    let macos = cfg!(target_os = "macos");
    let mut run = |program: &str, args: &[String]| service::run_command(program, args);
    let result = match action {
        ServiceAction::Install => service::install(&paths, macos, &mut run),
        ServiceAction::Uninstall => service::uninstall(&paths, macos, &mut run),
    };
    match result {
        Ok(lines) => {
            for line in lines {
                let _ = writeln!(out, "{}", plain.line(&line));
            }
            ExitCode::SUCCESS
        }
        Err(e) => failure(err, &e.to_string(), plain, false),
    }
}

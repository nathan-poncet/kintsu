use super::*;

struct Bench {
    dir: PathBuf,
}

impl Bench {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("kintsu-app-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::write(dir.join("bin").join("git"), "").unwrap();
        Self { dir }
    }

    fn run(&self, args: &[&str], session: Option<&str>) -> (ExitCode, String, String) {
        let rt = Runtime {
            args: args.iter().map(|s| s.to_string()).collect(),
            session: session.map(SessionId::new),
            cwd: Some(self.dir.display().to_string()),
            home: None,
            config_path: self.dir.join("config.toml"),
            state_dir: self.dir.join("state"),
            socket_path: self.dir.join("d.sock"),
            log_path: self.dir.join("d.log"),
            exe: PathBuf::from("/definitely/not/kintsu"),
            path_var: self.dir.join("bin").display().to_string(),
            color: false,
            tty_color: false,
            terminal_color: false,
            debug: true,
            daemon: false,
            terminal: TerminalIdentity::default(),
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run(&rt, &mut out, &mut err);
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }
}

impl Drop for Bench {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn a_typo_is_toasted_then_fixed_raw_for_the_shell_binding() {
    let b = Bench::new("typo");
    let (_, out, err) = b.run(
        &[
            "triage",
            "--status",
            "127",
            "--command",
            "gti status",
            "--session",
            "7",
            "--shell",
            "zsh",
        ],
        None,
    );
    assert!(out.is_empty());
    assert_eq!(
        err,
        "▎ Did you mean git status?\n▎ Tab to fix · kintsu why · kintsu agent · kintsu ignore · ^K more\n"
    );
    assert_eq!(
        std::fs::read_to_string(b.dir.join("state").join("sessions").join("7.ghost")).unwrap(),
        "git status"
    );
    let (code, out, _) = b.run(&["fix", "--raw"], Some("7"));
    assert_eq!((code, out.as_str()), (ExitCode::SUCCESS, "git status\n"));
    let (code, out, err) = b.run(&["fix"], Some("7"));
    assert_eq!(code, ExitCode::SUCCESS);
    assert!(out.starts_with("▎ git status\n"), "{out}{err}");
}

#[test]
fn success_is_silent_and_a_failure_without_fix_offers_the_commands() {
    let b = Bench::new("plain");
    let (code, out, err) = b.run(
        &[
            "triage",
            "--status",
            "0",
            "--command",
            "ls",
            "--session",
            "7",
        ],
        None,
    );
    assert_eq!(
        (code, out.as_str(), err.as_str()),
        (ExitCode::SUCCESS, "", "")
    );
    let (_, _, err) = b.run(
        &[
            "triage",
            "--status",
            "2",
            "--command",
            "make test",
            "--session",
            "7",
            "--duration-ms",
            "12000",
        ],
        None,
    );
    assert_eq!(
        err,
        "▎ make test exited 2 after 12 s.\n▎ kintsu fix · kintsu why · kintsu agent · kintsu ignore · ^K more\n"
    );
    let (code, _, err) = b.run(&["fix", "--raw"], Some("7"));
    assert_eq!((code, err.as_str()), (ExitCode::from(1), ""));
    let (code, _, err) = b.run(&["why"], Some("7"));
    assert_eq!(code, ExitCode::from(1));
    assert_eq!(err, "▎ kintsu: no model is configured for explanations\n");
}

#[test]
fn the_toast_is_remembered_so_the_panel_can_take_its_place() {
    let b = Bench::new("bubble");
    let (_, _, err) = b.run(
        &[
            "triage",
            "--status",
            "2",
            "--command",
            "make test",
            "--session",
            "7",
        ],
        None,
    );
    assert_eq!(
        std::fs::read_to_string(b.dir.join("state").join("sessions").join("7.bubble")).unwrap(),
        err.trim_end_matches('\n')
    );
}

#[test]
fn the_panel_stays_closed_once_the_shell_moved_on_or_never_failed() {
    let b = Bench::new("panel-closed");
    let fail = |status: &str, command: &str| {
        b.run(
            &[
                "triage",
                "--status",
                status,
                "--command",
                command,
                "--session",
                "7",
            ],
            None,
        );
    };
    fail("2", "make test");
    fail("0", "ls");
    assert_eq!(
        b.run(&["panel"], Some("7")),
        (ExitCode::SUCCESS, String::new(), String::new()),
        "the last command succeeded"
    );
    assert_eq!(
        b.run(&["panel"], Some("9")),
        (ExitCode::SUCCESS, String::new(), String::new()),
        "this shell never failed"
    );
}

#[test]
fn ignoring_the_command_keeps_the_next_identical_failure_quiet() {
    let b = Bench::new("ignore");
    b.run(
        &[
            "triage",
            "--status",
            "2",
            "--command",
            "make test",
            "--session",
            "7",
        ],
        None,
    );
    let (code, out, _) = b.run(&["ignore"], Some("7"));
    assert_eq!(code, ExitCode::SUCCESS);
    assert!(
        out.starts_with("▎ make test stays quiet everywhere."),
        "{out}"
    );
    b.run(
        &[
            "triage",
            "--status",
            "0",
            "--command",
            "ls",
            "--session",
            "7",
        ],
        None,
    );
    let (_, _, err) = b.run(
        &[
            "triage",
            "--status",
            "2",
            "--command",
            "make  test",
            "--session",
            "7",
        ],
        None,
    );
    assert_eq!(err, "");
    let (_, _, err) = b.run(
        &[
            "triage",
            "--status",
            "2",
            "--command",
            "make",
            "--session",
            "7",
        ],
        None,
    );
    assert!(err.starts_with("▎ make exited 2."));
    let (code, out, _) = b.run(&["mute", "1h"], Some("7"));
    assert_eq!(code, ExitCode::SUCCESS);
    assert!(out.starts_with("▎ Everything stays quiet until the mute ends."));
    let (_, _, err) = b.run(
        &[
            "triage",
            "--status",
            "1",
            "--command",
            "cargo test",
            "--session",
            "7",
        ],
        None,
    );
    assert_eq!(err, "");
}

#[test]
fn privacy_doctor_and_config_commands_answer() {
    let b = Bench::new("misc");
    b.run(
        &[
            "triage",
            "--status",
            "22",
            "--command",
            "curl -H 'Authorization: Bearer sk-live-abcdefghijklmnop' https://x",
            "--session",
            "7",
        ],
        None,
    );
    let (code, out, _) = b.run(&["privacy"], Some("7"));
    assert_eq!(code, ExitCode::SUCCESS);
    assert!(
        out.contains("(1 secret redacted)")
            && out.contains("Bearer ••••••••")
            && !out.contains("sk-live"),
        "{out}"
    );
    let (code, out, _) = b.run(&["doctor"], None);
    assert_eq!(code, ExitCode::SUCCESS);
    assert!(
        out.contains("✗ shell hook") && out.contains("! models"),
        "{out}"
    );
    let (_, out, _) = b.run(&["config", "path"], None);
    assert!(out.contains("config.toml  (missing") && out.contains("socket  "));
    let (_, out, _) = b.run(&["default-config"], None);
    assert_eq!(out, DEFAULT_CONFIG);
    let (code, _, err) = b.run(&["agent"], Some("7"));
    assert_eq!(code, ExitCode::from(1));
    assert!(err.contains("no agent is configured"));
}

#[test]
fn setup_with_yes_writes_a_file_from_what_the_path_offers() {
    let b = Bench::new("setup");
    for tool in ["ollama", "codex"] {
        std::fs::write(b.dir.join("bin").join(tool), "").unwrap();
    }
    let (code, out, err) = b.run(&["setup", "--yes"], Some("7"));
    assert_eq!(code, ExitCode::SUCCESS, "{err}");
    let written = std::fs::read_to_string(b.dir.join("config.toml")).unwrap();
    assert!(
        written.contains("[models.local]") && written.contains("[models.codex-cli]"),
        "{written}"
    );
    assert!(
        written.contains("investigate = [\"codex-cli\"]"),
        "{written}"
    );
    assert!(
        out.contains("written to") && out.contains("model codex-cli"),
        "{out}"
    );
    let (code, out, _) = b.run(&["doctor"], Some("7"));
    assert_eq!(code, ExitCode::SUCCESS);
    assert!(out.contains("model local"), "{out}");
}

#[test]
fn without_a_daemon_status_stop_and_pending_say_so_and_subscribe_leaves() {
    let b = Bench::new("nodaemon");
    let (code, out, _) = b.run(&["daemon", "status"], None);
    assert_eq!(code, ExitCode::from(1));
    assert!(out.contains("no daemon running"));
    let (code, out, _) = b.run(&["daemon", "stop"], None);
    assert_eq!(
        (code, out.as_str()),
        (ExitCode::SUCCESS, "▎ no daemon was running\n")
    );
    let (code, out, err) = b.run(&["pending"], Some("7"));
    assert_eq!(
        (code, out.as_str(), err.as_str()),
        (ExitCode::SUCCESS, "", "")
    );
    let (code, _, _) = b.run(&["subscribe", "--session", "7"], None);
    assert_eq!(code, ExitCode::from(1));
}

#[test]
fn a_broken_config_is_reported_and_triage_still_works() {
    let b = Bench::new("broken");
    std::fs::write(
        b.dir.join("config.toml"),
        "[models.x]\nprovider = \"nope\"\nmodel = \"m\"",
    )
    .unwrap();
    let (code, _, err) = b.run(
        &[
            "triage",
            "--status",
            "2",
            "--command",
            "make",
            "--session",
            "7",
        ],
        None,
    );
    assert_eq!(code, ExitCode::SUCCESS);
    assert!(
        err.starts_with("▎ kintsu: config: models.x: unknown provider `nope`"),
        "{err}"
    );
    assert!(err.contains("▎ make exited 2."));
    let (code, _, _) = b.run(&["why"], Some("7"));
    assert_eq!(code, ExitCode::from(2));
    let (code, _, err) = b.run(&["frobnicate"], None);
    assert_eq!(code, ExitCode::from(2));
    assert!(err.starts_with("kintsu: unknown command `frobnicate`"));
}

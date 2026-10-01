//! The resident process, end to end: the protocol over the socket, the
//! client that starts it, and a message delivered to a subscriber after a
//! model answered. The model is a tiny HTTP server in this test.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn kintsu() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kintsu"))
}

struct Fixture {
    dir: PathBuf,
    socket: PathBuf,
    child: Option<Child>,
}

impl Fixture {
    fn new(name: &str, config: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("kd-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::write(dir.join("bin").join("git"), "").unwrap();
        std::fs::write(dir.join("config.toml"), config).unwrap();
        let socket = dir.join("d.sock");
        Self {
            dir,
            socket,
            child: None,
        }
    }

    fn env(&self, cmd: &mut Command) {
        cmd.env("KINTSU_SOCKET", &self.socket)
            .env("KINTSU_STATE_DIR", self.dir.join("state"))
            .env("KINTSU_CONFIG", self.dir.join("config.toml"))
            .env("PATH", self.dir.join("bin"))
            .env("NO_COLOR", "1")
            .env_remove("KINTSU_SESSION")
            .env_remove("KINTSU_NO_DAEMON")
            .env_remove("KINTSU_DISABLE")
            .env_remove("KINTSU_TEST_KEY")
            .env_remove("LC_ALL")
            .env_remove("LC_MESSAGES")
            .env_remove("LANG");
        // The hook client would otherwise read the pane the tests run in.
        for identity in [
            "TERM_PROGRAM",
            "TMUX",
            "TMUX_PANE",
            "HERDR_PANE_ID",
            "HERDR_SOCKET_PATH",
            "HERDR_BIN_PATH",
            "WEZTERM_PANE",
            "KITTY_WINDOW_ID",
            "KITTY_LISTEN_ON",
            "ITERM_SESSION_ID",
        ] {
            cmd.env_remove(identity);
        }
    }

    fn start_daemon(&mut self) {
        let mut cmd = kintsu();
        cmd.args(["daemon", "run"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        self.env(&mut cmd);
        self.child = Some(cmd.spawn().unwrap());
        self.wait_for_socket(true);
    }

    fn wait_for_socket(&self, up: bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if UnixStream::connect(&self.socket).is_ok() == up {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!(
            "socket {} did not come {}",
            self.socket.display(),
            if up { "up" } else { "down" }
        );
    }

    /// Sends one frame and reads answers until the connection closes or a
    /// terminal frame arrives.
    fn exchange(&self, frame: &str) -> Vec<serde_json::Value> {
        let mut stream = UnixStream::connect(&self.socket).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream.write_all(frame.as_bytes()).unwrap();
        stream.write_all(b"\n").unwrap();
        let mut reader = BufReader::new(stream);
        let mut answers = Vec::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
            let v: serde_json::Value = serde_json::from_str(&line).unwrap();
            let kind = v["type"].as_str().unwrap_or_default().to_string();
            answers.push(v);
            if matches!(
                kind.as_str(),
                "decision" | "welcome" | "outdated" | "session" | "done" | "bye" | "ack" | "error"
            ) {
                break;
            }
        }
        answers
    }

    fn run(&self, args: &[&str], session: Option<&str>) -> (i32, String, String) {
        self.run_env(args, session, &[])
    }

    /// Like `run`, with variables only this process gets: what a shell has
    /// and the daemon does not.
    fn run_env(
        &self,
        args: &[&str],
        session: Option<&str>,
        extra: &[(&str, &str)],
    ) -> (i32, String, String) {
        let mut cmd = kintsu();
        cmd.args(args);
        self.env(&mut cmd);
        if let Some(s) = session {
            cmd.env("KINTSU_SESSION", s);
        }
        for (name, value) in extra {
            cmd.env(name, value);
        }
        let out = cmd.output().unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into(),
            String::from_utf8_lossy(&out.stderr).into(),
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if UnixStream::connect(&self.socket).is_ok() {
            let _ = self.exchange(r#"{"v":1,"type":"shutdown"}"#);
        }
        if let Some(child) = self.child.as_mut() {
            // An instrumented daemon writes its coverage profile as it
            // exits; killed mid-write, it leaves a corrupt file that fails
            // the whole merge. It was asked to stop: give it a moment.
            let deadline = Instant::now() + Duration::from_secs(3);
            while Instant::now() < deadline && matches!(child.try_wait(), Ok(None)) {
                std::thread::sleep(Duration::from_millis(20));
            }
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Answers every Ollama chat request with the given content.
fn fake_ollama(content: &'static str) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else { continue };
            let mut buf = vec![0u8; 65536];
            let mut read = 0;
            loop {
                let n = stream.read(&mut buf[read..]).unwrap_or(0);
                if n == 0 {
                    break;
                }
                read += n;
                let text = String::from_utf8_lossy(&buf[..read]).to_string();
                if let Some(split) = text.find("\r\n\r\n") {
                    let length: usize = text
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse().unwrap())
                        })
                        .unwrap_or(0);
                    if read >= split + 4 + length {
                        break;
                    }
                }
            }
            let body = format!(
                r#"{{"message":{{"role":"assistant","content":"{content}"}},"done":true}}"#
            );
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    port
}

/// An Ollama-shaped endpoint that records the body of every request and
/// answers with `content`, for what the prompt asked.
fn fake_ollama_recording(content: &'static str) -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&seen);
    std::thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else { continue };
            let mut buf = vec![0u8; 65536];
            let mut read = 0;
            loop {
                let n = stream.read(&mut buf[read..]).unwrap_or(0);
                if n == 0 {
                    break;
                }
                read += n;
                let text = String::from_utf8_lossy(&buf[..read]).to_string();
                if let Some(split) = text.find("\r\n\r\n") {
                    let length: usize = text
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse().unwrap())
                        })
                        .unwrap_or(0);
                    if read >= split + 4 + length {
                        break;
                    }
                }
            }
            let text = String::from_utf8_lossy(&buf[..read]).to_string();
            let body = text
                .split_once("\r\n\r\n")
                .map(|(_, b)| b.to_string())
                .unwrap_or_default();
            recorded.lock().unwrap().push(body);
            let body = format!(
                r#"{{"message":{{"role":"assistant","content":"{content}"}},"done":true}}"#
            );
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    (port, seen)
}

/// An Anthropic-shaped endpoint that records the `x-api-key` header of
/// every request and answers with `content`. Anthropic, not an
/// OpenAI-compatible one: a local endpoint of the latter needs no key.
fn fake_anthropic(content: &'static str) -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&seen);
    std::thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else { continue };
            let mut buf = vec![0u8; 65536];
            let mut read = 0;
            loop {
                let n = stream.read(&mut buf[read..]).unwrap_or(0);
                if n == 0 {
                    break;
                }
                read += n;
                let text = String::from_utf8_lossy(&buf[..read]).to_string();
                if let Some(split) = text.find("\r\n\r\n") {
                    let length: usize = text
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse().unwrap())
                        })
                        .unwrap_or(0);
                    if read >= split + 4 + length {
                        break;
                    }
                }
            }
            let text = String::from_utf8_lossy(&buf[..read]).to_string();
            let key = text
                .lines()
                .find(|l| l.to_ascii_lowercase().starts_with("x-api-key:"))
                .and_then(|l| l.split_once(':'))
                .map(|(_, v)| v.trim().to_string())
                .unwrap_or_default();
            recorded.lock().unwrap().push(key);
            let body = format!(r#"{{"content":[{{"type":"text","text":"{content}"}}]}}"#);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    (port, seen)
}

/// The first bubble the daemon has for the session, within ten seconds.
fn wait_for_bubble(f: &Fixture, session: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        let frames = f.exchange(&format!(
            r#"{{"v":1,"type":"pending","session":"{session}"}}"#
        ));
        if let Some(bubble) = frames.iter().find(|frame| frame["type"] == "bubble") {
            return bubble["text"].as_str().unwrap_or("").to_string();
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("no bubble for {session} within ten seconds");
}

#[test]
fn the_daemon_speaks_the_protocol() {
    let mut f = Fixture::new("protocol", "[ui]\neager_fix = false\n");
    f.start_daemon();
    let hello = f.exchange(&format!(
        r#"{{"v":1,"type":"hello","version":"{}"}}"#,
        env!("CARGO_PKG_VERSION")
    ));
    assert_eq!(hello[0]["type"], "welcome");
    assert_eq!(hello[0]["version"], env!("CARGO_PKG_VERSION"));

    let typo = f.exchange(r#"{"v":1,"type":"command_finished","session":"s1","command":"gti status","status":127,"cwd":"/","shell":"zsh"}"#);
    assert_eq!(typo[0]["type"], "decision");
    assert_eq!(typo[0]["offer"]["fix"], "git status");
    assert_eq!(typo[0]["ghost"], "git status", "safe enough to pre-type");
    assert!(
        typo[0]["toast"].as_str().unwrap().contains("Tab to fix"),
        "{}",
        typo[0]["toast"]
    );
    assert!(
        typo[0]["toast"]
            .as_str()
            .unwrap()
            .contains("Did you mean git status?"),
        "{}",
        typo[0]
    );

    let ok =
        f.exchange(r#"{"v":1,"type":"command_finished","session":"s1","command":"ls","status":0}"#);
    assert_eq!(ok[0]["quiet"], "succeeded");

    // The rules look at the PATH the frame carries, not the daemon's own:
    // under launchd the daemon's is the bare system one. Another session,
    // so s1's last failure stays the one `fix` reads below.
    let shell_bin = f.dir.join("shell-bin");
    std::fs::create_dir_all(&shell_bin).unwrap();
    std::fs::write(shell_bin.join("frobnicate"), "").unwrap();
    let theirs = f.exchange(&format!(
        r#"{{"v":1,"type":"command_finished","session":"sp","command":"frobnicat","status":127,"cwd":"/","shell":"zsh","path":"{}"}}"#,
        shell_bin.display()
    ));
    assert_eq!(theirs[0]["offer"]["fix"], "frobnicate");
    let not_theirs = f.exchange(&format!(
        r#"{{"v":1,"type":"command_finished","session":"sp","command":"gti log","status":127,"cwd":"/","shell":"zsh","path":"{}"}}"#,
        shell_bin.display()
    ));
    assert_eq!(
        not_theirs[0]["offer"]["fix"],
        serde_json::Value::Null,
        "git is on the daemon's PATH, not on this shell's"
    );

    // A shell that registered at start: a later frame that omits the PATH
    // still gets the shell's, from the registration.
    let registered = f.exchange(&format!(
        r#"{{"v":1,"type":"session_new","session":"sn","shell":"zsh","pid":{},"tty":"/dev/ttys009","path":"{}","terminal":{{"program":"ghostty"}}}}"#,
        std::process::id(),
        shell_bin.display()
    ));
    assert_eq!(
        (
            registered[0]["type"].as_str(),
            registered[0]["session"].as_str()
        ),
        (Some("session"), Some("sn"))
    );
    let from_registration = f.exchange(
        r#"{"v":1,"type":"command_finished","session":"sn","command":"frobnicat","status":127,"cwd":"/","shell":"zsh"}"#,
    );
    assert_eq!(
        from_registration[0]["offer"]["fix"], "frobnicate",
        "the PATH the shell registered with"
    );
    // The command the hooks run at start, with the client's own PATH.
    let pid = std::process::id().to_string();
    let (code, out, err) = f.run(
        &["session", "new", "--shell", "zsh", "--pid", &pid],
        Some("sn2"),
    );
    assert_eq!((code, out.as_str(), err.as_str()), (0, "", ""));
    let via_cli = f.exchange(
        r#"{"v":1,"type":"command_finished","session":"sn2","command":"gti log","status":127,"cwd":"/","shell":"zsh"}"#,
    );
    assert_eq!(
        via_cli[0]["offer"]["fix"], "git log",
        "the client's PATH has git"
    );

    let pending = f.exchange(r#"{"v":1,"type":"pending","session":"s1"}"#);
    assert_eq!(pending.last().unwrap()["type"], "done");
    assert_eq!(pending.len(), 1, "nothing was waiting");

    let bad = f.exchange(r#"{"v":1,"type":"dance"}"#);
    assert_eq!(bad[0]["type"], "error");

    let (code, out, _) = f.run(&["fix", "--raw"], Some("s1"));
    assert_eq!(
        (code, out.as_str()),
        (0, "git status\n"),
        "the case the daemon saved is the one the client reads"
    );

    // The output is read from the terminal after the bubble: here a fake
    // tmux answers capture-pane with a screen dump.
    std::fs::write(
        f.dir.join("bin").join("tmux"),
        "#!/bin/sh\nprintf '$ make test\\nmake: *** No rule to make target test.  Stop.\\n'\n",
    )
    .unwrap();
    std::fs::set_permissions(
        f.dir.join("bin").join("tmux"),
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    f.exchange(r#"{"v":1,"type":"command_finished","session":"s1","command":"make test","status":2,"cwd":"/","terminal":{"tmux_pane":"%1"}}"#);
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut privacy = String::new();
    while Instant::now() < deadline && !privacy.contains("## Output") {
        std::thread::sleep(Duration::from_millis(50));
        privacy = f.run(&["privacy"], Some("s1")).1;
    }
    assert!(
        privacy
            .contains("## Output\n\n```text\nmake: *** No rule to make target test.  Stop.\n```"),
        "{privacy}"
    );

    // A rule that reads the output answers before any model, and here no
    // model is configured at all: the fix still arrives as a bubble.
    std::fs::write(
        f.dir.join("bin").join("tmux"),
        "#!/bin/sh\nprintf '$ touch /etc/hosts.new\\ntouch: /etc/hosts.new: Permission denied\\n'\n",
    )
    .unwrap();
    f.exchange(r#"{"v":1,"type":"command_finished","session":"s1","command":"touch /etc/hosts.new","status":1,"cwd":"/","terminal":{"tmux_pane":"%1"}}"#);
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut bubbles: Vec<serde_json::Value> = Vec::new();
    while Instant::now() < deadline && bubbles.is_empty() {
        std::thread::sleep(Duration::from_millis(50));
        bubbles = f
            .exchange(r#"{"v":1,"type":"pending","session":"s1"}"#)
            .into_iter()
            .filter(|frame| frame["type"] == "bubble")
            .collect();
    }
    assert!(
        bubbles.iter().any(|b| b["text"]
            .as_str()
            .unwrap_or("")
            .contains("sudo touch /etc/hosts.new")),
        "{bubbles:?}"
    );
    let (code, out, _) = f.run(&["fix", "--raw"], Some("s1"));
    assert_eq!((code, out.as_str()), (0, "sudo touch /etc/hosts.new\n"));

    let bye = f.exchange(r#"{"v":1,"type":"shutdown"}"#);
    assert_eq!(bye[0]["type"], "bye");
    f.wait_for_socket(false);
    let status = f.child.as_mut().unwrap().wait().unwrap();
    assert!(status.success());
}

#[test]
fn the_hook_client_starts_the_daemon_and_the_daemon_commands_talk_to_it() {
    let f = Fixture::new("spawn", "[ui]\neager_fix = false\n");
    let (code, _, err) = f.run(
        &[
            "triage",
            "--status",
            "127",
            "--command",
            "gti status",
            "--session",
            "s2",
        ],
        None,
    );
    assert_eq!(code, 0);
    assert!(err.contains("Did you mean git status?"), "{err}");
    f.wait_for_socket(true);
    let (code, out, _) = f.run(&["daemon", "status"], None);
    assert_eq!(code, 0);
    assert!(
        out.contains(&format!("daemon {} running", env!("CARGO_PKG_VERSION"))),
        "{out}"
    );
    let (_, _, err) = f.run(
        &[
            "triage",
            "--status",
            "2",
            "--command",
            "make",
            "--session",
            "s2",
        ],
        None,
    );
    assert!(err.starts_with("▎ make exited 2."), "{err}");
    let (code, out, _) = f.run(&["daemon", "stop"], None);
    assert_eq!((code, out.as_str()), (0, "▎ daemon stopped\n"));
    f.wait_for_socket(false);
}

#[test]
fn a_subscriber_receives_the_models_fix_as_a_message_and_fix_reuses_it() {
    let port = fake_ollama("make -j4 test");
    let config = format!(
        "[models.local]\nprovider = \"ollama\"\nmodel = \"m\"\nbase_url = \"http://127.0.0.1:{port}\"\n[routing]\nquick_fix = [\"local\"]\nexplain = [\"local\"]\n[ui]\neager_fix = true\n"
    );
    let mut f = Fixture::new("message", &config);
    f.start_daemon();

    let mut subscriber = UnixStream::connect(&f.socket).unwrap();
    subscriber
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    subscriber
        .write_all(br#"{"v":1,"type":"subscribe","session":"s3","color":false}"#)
        .unwrap();
    subscriber.write_all(b"\n").unwrap();
    let mut reader = BufReader::new(subscriber);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    assert!(line.contains(r#""type":"ack""#), "{line}");

    let decision = f.exchange(r#"{"v":1,"type":"command_finished","session":"s3","command":"make test","status":2,"cwd":"/"}"#);
    assert_eq!(decision[0]["offer"]["fix"], serde_json::Value::Null);
    assert_eq!(
        decision[0]["pending"], "local",
        "the model being asked, for the asking… line"
    );
    assert!(
        decision[0]["toast"]
            .as_str()
            .unwrap()
            .ends_with("▎ asking local…"),
        "{}",
        decision[0]["toast"]
    );

    line.clear();
    reader.read_line(&mut line).unwrap();
    let bubble: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(bubble["type"], "bubble", "{line}");
    let text = bubble["text"].as_str().unwrap();
    assert!(
        text.contains("Try make -j4 test?") && text.contains("^K more"),
        "{text}"
    );

    let (code, out, _) = f.run(&["fix", "--raw"], Some("s3"));
    assert_eq!((code, out.as_str()), (0, "make -j4 test\n"));

    let pending = f.exchange(r#"{"v":1,"type":"pending","session":"s3"}"#);
    assert_eq!(pending.len(), 1, "delivered live, nothing left pending");

    // `kintsu why` through the daemon: an immediate "asking…" line, a marker
    // for the hook, and the explanation as a bubble on the subscription.
    let (code, _, err) = f.run(&["why"], Some("s3"));
    assert_eq!((code, err.as_str()), (0, "▎ asking local…\n"));
    assert!(
        f.dir
            .join("state")
            .join("sessions")
            .join("s3.asking")
            .is_file(),
        "marker for the hook"
    );
    line.clear();
    reader.read_line(&mut line).unwrap();
    let bubble: serde_json::Value = serde_json::from_str(&line).unwrap();
    let text = bubble["text"].as_str().unwrap();
    assert!(
        text.contains("make -j4 test") && text.contains("— local"),
        "{text}"
    );

    // A click on a word: the desktop runs `kintsu open kintsu://act?…`; the
    // daemon answers in the shell the case came from, never elsewhere.
    let case = decision[0]["offer"]["case"].as_str().unwrap().to_string();
    let (code, _, err) = f.run(&["open", &format!("kintsu://act?case={case}&do=why")], None);
    assert_eq!((code, err.as_str()), (0, ""));
    line.clear();
    reader.read_line(&mut line).unwrap();
    let bubble: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert!(
        bubble["text"].as_str().unwrap().contains("— local"),
        "the click's explanation lands on the subscription: {line}"
    );
    let (code, _, err) = f.run(&["open", &format!("kintsu://act?case={case}&do=fix")], None);
    assert_eq!((code, err.as_str()), (0, ""));
    line.clear();
    reader.read_line(&mut line).unwrap();
    let bubble: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert!(
        bubble["text"]
            .as_str()
            .unwrap()
            .contains("Try make -j4 test?"),
        "a click on fix sends the stored proposal: {line}"
    );
    let (code, _, err) = f.run(
        &["open", &format!("kintsu://act?case={case}&do=ignore")],
        None,
    );
    assert_eq!((code, err.as_str()), (0, ""));
    line.clear();
    reader.read_line(&mut line).unwrap();
    let bubble: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert!(
        bubble["text"].as_str().unwrap().contains("make test"),
        "a click on ignore says what is now quiet: {line}"
    );
    let (code, _, err) = f.run(&["open", "kintsu://act?case=gone&do=why"], None);
    assert_eq!(code, 1);
    assert!(err.contains("this case is gone"), "{err}");
    let (code, _, err) = f.run(&["open", "https://example.com"], None);
    assert_eq!(code, 1);
    assert!(err.contains("not a kintsu:// URL"), "{err}");

    let refused = f.exchange(r#"{"v":1,"type":"explain","session":"nobody"}"#);
    assert_eq!(refused[0]["type"], "error");
    assert!(
        refused[0]["message"]
            .as_str()
            .unwrap()
            .contains("no failure"),
        "{}",
        refused[0]
    );
}

#[test]
fn a_key_the_shell_sees_reaches_the_model_through_a_daemon_that_has_none() {
    let (port, seen) = fake_anthropic("cargo build --release");
    let config = format!(
        "[models.cloud]\nprovider = \"anthropic\"\nmodel = \"m\"\nbase_url = \"http://127.0.0.1:{port}\"\nkey = {{ env = \"KINTSU_TEST_KEY\" }}\n[routing]\nquick_fix = [\"cloud\"]\n[ui]\neager_fix = true\n"
    );
    let mut f = Fixture::new("keys", &config);
    f.start_daemon();

    // The daemon was started without the variable: on its own it has no key.
    f.exchange(r#"{"v":1,"type":"command_finished","session":"s6","command":"make test","status":2,"cwd":"/"}"#);
    let refused = wait_for_bubble(&f, "s6");
    assert!(refused.contains("needs a key"), "{refused}");
    assert!(
        seen.lock().unwrap().is_empty(),
        "nothing was sent without a key"
    );

    // The hook's process has it, as a shell would: it travels with the frame.
    let (code, _, err) = f.run_env(
        &[
            "triage",
            "--status",
            "2",
            "--command",
            "make test",
            "--session",
            "s7",
        ],
        None,
        &[("KINTSU_TEST_KEY", "sk-from-shell")],
    );
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("asking cloud"), "{err}");
    let fixed = wait_for_bubble(&f, "s7");
    assert!(fixed.contains("cargo build --release"), "{fixed}");
    assert_eq!(seen.lock().unwrap().as_slice(), ["sk-from-shell"]);
}

#[test]
fn the_shells_language_reaches_the_models_prompt_and_a_silent_shell_gets_english() {
    let (port, bodies) = fake_ollama_recording("make -j4 test");
    let config = format!(
        "[models.local]\nprovider = \"ollama\"\nmodel = \"m\"\nbase_url = \"http://127.0.0.1:{port}\"\n[routing]\nquick_fix = [\"local\"]\n[ui]\neager_fix = true\n"
    );
    let mut f = Fixture::new("language", &config);
    f.start_daemon();
    // The hook's process reads its locale and names the language in the frame.
    let (code, _, err) = f.run_env(
        &[
            "triage",
            "--status",
            "2",
            "--command",
            "make test",
            "--session",
            "s9",
        ],
        None,
        &[("LANG", "fr_FR.UTF-8")],
    );
    assert_eq!(code, 0, "{err}");
    let fixed = wait_for_bubble(&f, "s9");
    assert!(fixed.contains("make -j4 test"), "{fixed}");
    // The reachability probe has no body; the chat request has the prompt.
    let chats = |from: usize| -> Vec<String> {
        bodies.lock().unwrap()[from..]
            .iter()
            .filter(|b| b.contains("\"messages\""))
            .cloned()
            .collect()
    };
    let first = chats(0);
    assert!(
        first.iter().any(|b| b.contains("The user reads French")),
        "{first:?}"
    );
    let seen = bodies.lock().unwrap().len();
    // A frame naming no language, from a shell that registered none: the
    // daemon's own locale, which the fixture leaves empty: English.
    f.exchange(
        r#"{"v":1,"type":"command_finished","session":"s10","command":"make test","status":2,"cwd":"/"}"#,
    );
    wait_for_bubble(&f, "s10");
    let second = chats(seen);
    assert!(!second.is_empty(), "the model was asked again");
    assert!(second.iter().all(|b| !b.contains("French")), "{second:?}");
}

#[test]
fn without_a_subscriber_the_message_waits_and_comes_with_the_next_decision_on_the_same_failure() {
    let port = fake_ollama("cargo build --release");
    let config = format!(
        "[models.local]\nprovider = \"ollama\"\nmodel = \"m\"\nbase_url = \"http://127.0.0.1:{port}\"\n[routing]\nquick_fix = [\"local\"]\n[ui]\neager_fix = true\n"
    );
    let mut f = Fixture::new("pending", &config);
    f.start_daemon();
    let (code, _, err) = f.run(
        &[
            "triage",
            "--status",
            "2",
            "--command",
            "make test",
            "--session",
            "s4",
        ],
        None,
    );
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("asking local"), "{err}");
    assert!(
        f.dir
            .join("state")
            .join("sessions")
            .join("s4.asking")
            .is_file(),
        "the marker the hook reads"
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut bubbles = Vec::new();
    while Instant::now() < deadline && bubbles.is_empty() {
        std::thread::sleep(Duration::from_millis(50));
        // The same failure again: the shell still looks at it, so what
        // waited is shown. After another command it would be dropped.
        let next = f.exchange(
            r#"{"v":1,"type":"command_finished","session":"s4","command":"make test","status":2}"#,
        );
        bubbles = next[0]["bubbles"].as_array().cloned().unwrap_or_default();
    }
    assert_eq!(
        bubbles.len(),
        1,
        "the message arrived with a later decision"
    );
    assert!(
        bubbles[0]
            .as_str()
            .unwrap()
            .contains("Try cargo build --release?")
    );
}

#[test]
fn the_shells_copy_of_stderr_feeds_the_capture_and_the_rules_that_read_it() {
    let mut f = Fixture::new(
        "stderr",
        "[capture]\nstderr_tee = true\n[ui]\neager_fix = false\n",
    );
    f.start_daemon();
    let (code, hook, _) = f.run(&["init", "zsh"], None);
    assert_eq!(code, 0);
    assert!(
        hook.contains("__kintsu_stderr_tee=\"1\""),
        "the hook copies stderr only when the option is on"
    );
    // The hook wrote the copy; the frame names it; no pane reader exists.
    let copy = f.dir.join("state").join("sessions").join("s6.stderr");
    std::fs::create_dir_all(copy.parent().unwrap()).unwrap();
    std::fs::write(&copy, "touch: /etc/hosts.new: Permission denied\n").unwrap();
    f.exchange(&format!(
        r#"{{"v":1,"type":"command_finished","session":"s6","command":"touch /etc/hosts.new","status":1,"cwd":"/","shell":"zsh","terminal":{{"stderr_copy":"{}"}}}}"#,
        copy.display()
    ));
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut bubbles: Vec<serde_json::Value> = Vec::new();
    while Instant::now() < deadline && bubbles.is_empty() {
        std::thread::sleep(Duration::from_millis(50));
        bubbles = f
            .exchange(r#"{"v":1,"type":"pending","session":"s6"}"#)
            .into_iter()
            .filter(|frame| frame["type"] == "bubble")
            .collect();
    }
    assert!(
        bubbles.iter().any(|b| b["text"]
            .as_str()
            .unwrap_or("")
            .contains("sudo touch /etc/hosts.new")),
        "{bubbles:?}"
    );
    assert!(!copy.exists(), "the copy is read once");
    let (code, privacy, _) = f.run(&["privacy"], Some("s6"));
    assert_eq!(code, 0);
    assert!(
        privacy.contains("touch: /etc/hosts.new: Permission denied"),
        "{privacy}"
    );
    let (_, doctor, _) = f.run(&["doctor"], Some("s6"));
    assert!(
        doctor.contains("zsh copies each command's stderr"),
        "{doctor}"
    );
}

#[test]
fn a_fix_taken_twice_becomes_an_instant_rule_and_the_model_is_not_asked_again() {
    let port = fake_ollama("make -j4 test");
    let config = format!(
        "[models.local]\nprovider = \"ollama\"\nmodel = \"m\"\nbase_url = \"http://127.0.0.1:{port}\"\n[routing]\nquick_fix = [\"local\"]\n[ui]\neager_fix = true\n"
    );
    let mut f = Fixture::new("learned", &config);
    f.start_daemon();
    for _ in 0..2 {
        let offered = f.exchange(
            r#"{"v":1,"type":"command_finished","session":"s5","command":"make test","status":2,"cwd":"/"}"#,
        );
        assert_eq!(offered[0]["offer"]["fix"], serde_json::Value::Null);
        assert_eq!(offered[0]["pending"], "local", "the model is asked");
        // The model's fix is kept with the case before it is delivered; the
        // bubble waiting for the shell says it landed.
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut landed = false;
        while Instant::now() < deadline && !landed {
            std::thread::sleep(Duration::from_millis(50));
            landed = f
                .exchange(r#"{"v":1,"type":"pending","session":"s5"}"#)
                .iter()
                .any(|frame| {
                    frame["text"]
                        .as_str()
                        .is_some_and(|t| t.contains("make -j4 test"))
                });
        }
        assert!(landed, "the model's fix never arrived");
        let taken = f.exchange(
            r#"{"v":1,"type":"command_finished","session":"s5","command":"make -j4 test","status":0,"cwd":"/"}"#,
        );
        assert_eq!(taken[0]["quiet"], "succeeded");
    }
    let third = f.exchange(
        r#"{"v":1,"type":"command_finished","session":"s5","command":"make test","status":2,"cwd":"/"}"#,
    );
    assert_eq!(
        third[0]["offer"]["fix"], "make -j4 test",
        "taken twice: a rule now"
    );
    assert_eq!(
        third[0]["pending"],
        serde_json::Value::Null,
        "no model asked"
    );
    let (code, out, _) = f.run(&["learned"], None);
    assert_eq!(code, 0);
    assert!(
        out.contains("make test exited 2: make -j4 test") && out.contains("taken 2 times"),
        "{out}"
    );
    let (code, out, _) = f.run(&["learned", "forget", "make"], None);
    assert_eq!(
        (code, out.as_str()),
        (0, "▎ Forgot 1 learned fix for make.\n")
    );
}

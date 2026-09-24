//! The resident process, end to end: the protocol over the socket, the
//! client that starts it, and a message delivered to a subscriber after a
//! model answered. The model is a tiny HTTP server in this test.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
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
            .env_remove("KINTSU_DISABLE");
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
                "decision" | "welcome" | "outdated" | "done" | "bye" | "ack" | "error"
            ) {
                break;
            }
        }
        answers
    }

    fn run(&self, args: &[&str], session: Option<&str>) -> (i32, String, String) {
        let mut cmd = kintsu();
        cmd.args(args);
        self.env(&mut cmd);
        if let Some(s) = session {
            cmd.env("KINTSU_SESSION", s);
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
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Answers one Ollama chat request with the given content, then exits.
fn fake_ollama(content: &'static str) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
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
        let body =
            format!(r#"{{"message":{{"role":"assistant","content":"{content}"}},"done":true}}"#);
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
    });
    port
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
        "[models.local]\nprovider = \"ollama\"\nmodel = \"m\"\nbase_url = \"http://127.0.0.1:{port}\"\n[routing]\nquick_fix = [\"local\"]\n[ui]\neager_fix = true\n"
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

    line.clear();
    reader.read_line(&mut line).unwrap();
    let bubble: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(bubble["type"], "bubble", "{line}");
    let text = bubble["text"].as_str().unwrap();
    assert!(
        text.contains("Try make -j4 test?") && text.contains("^K to insert"),
        "{text}"
    );

    let (code, out, _) = f.run(&["fix", "--raw"], Some("s3"));
    assert_eq!((code, out.as_str()), (0, "make -j4 test\n"));

    let pending = f.exchange(r#"{"v":1,"type":"pending","session":"s3"}"#);
    assert_eq!(pending.len(), 1, "delivered live, nothing left pending");
}

#[test]
fn without_a_subscriber_the_message_waits_and_comes_with_the_next_decision() {
    let port = fake_ollama("cargo build --release");
    let config = format!(
        "[models.local]\nprovider = \"ollama\"\nmodel = \"m\"\nbase_url = \"http://127.0.0.1:{port}\"\n[routing]\nquick_fix = [\"local\"]\n[ui]\neager_fix = true\n"
    );
    let mut f = Fixture::new("pending", &config);
    f.start_daemon();
    f.exchange(r#"{"v":1,"type":"command_finished","session":"s4","command":"make test","status":2,"cwd":"/"}"#);
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut bubbles = Vec::new();
    while Instant::now() < deadline && bubbles.is_empty() {
        std::thread::sleep(Duration::from_millis(50));
        let next = f.exchange(
            r#"{"v":1,"type":"command_finished","session":"s4","command":"ls","status":0}"#,
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

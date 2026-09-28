//! `kintsu models` and `kintsu login` through the real binary, against a
//! keychain faked by a `security` (or `secret-tool`) script on a scratch
//! PATH: the key goes in, is found, and never shows.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const FAKE_SECURITY: &str = r#"#!/bin/sh
# a keychain in files: add-generic-password stores, find-generic-password prints
PATH="/usr/bin:/bin:$PATH"
dir="${FAKE_KEYCHAIN_DIR:?}"
cmd="$1"; shift
acct=""; key=""
while [ $# -gt 0 ]; do
  case "$1" in
    -a) acct="$2"; shift ;;
    -w) if [ $# -gt 1 ]; then key="$2"; shift; fi ;;
  esac
  shift
done
case "$cmd" in
  add-generic-password) printf '%s' "$key" > "$dir/$acct" ;;
  find-generic-password) cat "$dir/$acct" 2>/dev/null || exit 44 ;;
  *) exit 2 ;;
esac
"#;

const FAKE_SECRET_TOOL: &str = r#"#!/bin/sh
PATH="/usr/bin:/bin:$PATH"
dir="${FAKE_KEYCHAIN_DIR:?}"
cmd="$1"; shift
acct=""
while [ $# -gt 0 ]; do
  case "$1" in account) acct="$2"; shift ;; esac
  shift
done
case "$cmd" in
  store) cat > "$dir/$acct" ;;
  lookup) cat "$dir/$acct" 2>/dev/null || exit 1 ;;
  *) exit 2 ;;
esac
"#;

const CONFIG: &str = r#"# written by hand

[models.local]
provider = "ollama"
model    = "m"
base_url = "http://127.0.0.1:1"

[models.haiku]
provider = "anthropic"
model    = "claude-haiku-4-5"
key      = { keychain = true }

[models.cloud]
provider = "anthropic"
model    = "claude-sonnet-5"
key      = { env = "KINTSU_TEST_SURELY_UNSET" }

[routing]
explain = ["haiku"]
"#;

struct Fixture {
    dir: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("km-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::create_dir_all(dir.join("keychain")).unwrap();
        for (name, body) in [
            ("security", FAKE_SECURITY),
            ("secret-tool", FAKE_SECRET_TOOL),
        ] {
            let path = dir.join("bin").join(name);
            std::fs::write(&path, body).unwrap();
            std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o755))
                .unwrap();
        }
        std::fs::write(dir.join("config.toml"), CONFIG).unwrap();
        Self { dir }
    }

    fn run(&self, args: &[&str], stdin: Option<&str>) -> (i32, String, String) {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_kintsu"));
        cmd.args(args)
            .env("KINTSU_CONFIG", self.dir.join("config.toml"))
            .env("KINTSU_STATE_DIR", self.dir.join("state"))
            .env("KINTSU_NO_DAEMON", "1")
            .env("FAKE_KEYCHAIN_DIR", self.dir.join("keychain"))
            .env("PATH", self.dir.join("bin"))
            .env("NO_COLOR", "1")
            .env_remove("KINTSU_SESSION")
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().unwrap();
        if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
            pipe.write_all(text.as_bytes()).unwrap();
        }
        let out = child.wait_with_output().unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into(),
            String::from_utf8_lossy(&out.stderr).into(),
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn login_puts_the_key_in_the_keychain_and_models_finds_it_without_showing_it() {
    let f = Fixture::new("login");
    let (code, out, _) = f.run(&["models"], None);
    assert_eq!(code, 0);
    assert!(
        out.contains("haiku") && out.contains("no keychain entry kintsu/haiku"),
        "{out}"
    );
    assert!(out.contains("server not running"), "{out}");
    assert!(
        out.contains("$KINTSU_TEST_SURELY_UNSET is not set"),
        "{out}"
    );

    let (code, out, err) = f.run(&["models", "test"], None);
    assert_eq!(code, 1, "a model that should answer did not: {out}{err}");
    assert!(out.contains("haiku") && out.contains("no key"), "{out}");

    let (code, out, err) = f.run(&["login", "haiku"], Some("sk-test-123\n"));
    assert_eq!(code, 0, "{err}");
    assert!(
        !out.contains("sk-test") && !err.contains("sk-test"),
        "the key never shows: {out}{err}"
    );
    assert!(
        out.contains("stored in the keychain") && out.contains("already reads it"),
        "{out}"
    );
    assert_eq!(
        std::fs::read_to_string(f.dir.join("keychain").join("haiku")).unwrap(),
        "sk-test-123"
    );

    let (_, out, _) = f.run(&["models"], None);
    assert!(out.contains("key in the keychain (haiku)"), "{out}");
    let (_, json, _) = f.run(&["models", "--json"], None);
    let parsed: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
    let haiku = parsed
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["name"] == "haiku")
        .unwrap();
    assert_eq!(haiku["key"]["found"], true);
    assert_eq!(haiku["key"]["source"], "keychain:haiku");
    assert!(!json.contains("sk-test"));
}

#[test]
fn login_can_point_the_configuration_at_the_keychain_and_refuses_what_makes_no_sense() {
    let f = Fixture::new("write");
    let (code, out, err) = f.run(&["login", "cloud", "--write-config"], Some("sk-cloud\n"));
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("now reads it"), "{out}");
    let written = std::fs::read_to_string(f.dir.join("config.toml")).unwrap();
    assert!(
        written.contains("[models.cloud]\nprovider = \"anthropic\"\nmodel    = \"claude-sonnet-5\"\nkey      = { keychain = true }\n"),
        "{written}"
    );
    assert!(
        written.starts_with("# written by hand") && written.contains("[models.local]"),
        "the rest of the file is intact: {written}"
    );
    assert!(!written.contains("sk-cloud"), "no key in the file");

    let (code, _, err) = f.run(&["login", "local"], Some("k\n"));
    assert_eq!(code, 1);
    assert!(err.contains("needs no key"), "{err}");
    let (code, _, err) = f.run(&["login", "nope"], Some("k\n"));
    assert_eq!(code, 1);
    assert!(err.contains("not a configured model"), "{err}");
    let (code, _, err) = f.run(&["login", "haiku"], Some("   \n"));
    assert_eq!(code, 1);
    assert!(err.contains("the key is empty"), "{err}");
}

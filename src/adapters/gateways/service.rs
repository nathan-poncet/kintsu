//! `kintsu service install`: the daemon kept alive by launchd or systemd,
//! and the `kintsu://` scheme handed to `kintsu open` by the desktop.
//! The files are pure functions of the binary's path; the commands that
//! register them run through a runner, so tests see what would run.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const SCHEME: &str = "kintsu";
pub const LAUNCHD_LABEL: &str = "dev.kintsu.daemon";

/// Runs a program with arguments; the real one calls it, tests record it.
pub type Runner<'a> = &'a mut dyn FnMut(&str, &[String]) -> io::Result<()>;

pub fn run_command(program: &str, args: &[String]) -> io::Result<()> {
    let status = Command::new(program).args(args).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("{program} exited with {status}")))
    }
}

/// The launchd agent that keeps the daemon running after logout.
pub fn launch_agent_plist(exe: &Path, log: &Path) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LAUNCHD_LABEL}</string>
  <key>ProgramArguments</key>
  <array><string>{exe}</string><string>daemon</string><string>run</string></array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardOutPath</key><string>{log}</string>
  <key>StandardErrorPath</key><string>{log}</string>
</dict>
</plist>
"#,
        exe = exe.display(),
        log = log.display()
    )
}

/// The systemd user unit for the same job.
pub fn systemd_unit(exe: &Path) -> String {
    format!(
        "[Unit]\nDescription=kintsu daemon\n\n[Service]\nExecStart={} daemon run\nRestart=on-failure\n\n[Install]\nWantedBy=default.target\n",
        exe.display()
    )
}

/// The AppleScript of a tiny app that receives `kintsu://` URLs and hands
/// them to `kintsu open`. macOS delivers URLs as an event, not as argv,
/// which is why a script app stands in between.
pub fn url_handler_applescript(exe: &Path) -> String {
    format!(
        "on open location this_url\n    do shell script quoted form of \"{}\" & \" open \" & quoted form of this_url\nend open location\n",
        exe.display()
    )
}

/// The desktop entry that makes `kintsu open` the handler of the scheme.
pub fn desktop_entry(exe: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=Kintsu\nExec={} open %u\nNoDisplay=true\nMimeType=x-scheme-handler/{SCHEME};\n",
        exe.display()
    )
}

/// Where the pieces go, per system.
pub struct ServicePaths {
    pub home: PathBuf,
    pub state_dir: PathBuf,
    pub exe: PathBuf,
}

impl ServicePaths {
    pub fn launch_agent(&self) -> PathBuf {
        self.home
            .join("Library/LaunchAgents")
            .join(format!("{LAUNCHD_LABEL}.plist"))
    }

    pub fn url_app(&self) -> PathBuf {
        self.home.join("Applications/Kintsu.app")
    }

    pub fn systemd_unit(&self) -> PathBuf {
        self.home.join(".config/systemd/user/kintsu.service")
    }

    pub fn desktop_entry(&self) -> PathBuf {
        self.home.join(".local/share/applications/kintsu.desktop")
    }
}

/// Installs the daemon service and the scheme handler; returns what was
/// done, one line each.
pub fn install(paths: &ServicePaths, macos: bool, run: Runner<'_>) -> io::Result<Vec<String>> {
    let mut done = Vec::new();
    let s = |v: &str| v.to_string();
    if macos {
        let plist = paths.launch_agent();
        write(
            &plist,
            &launch_agent_plist(&paths.exe, &paths.state_dir.join("daemon.log")),
        )?;
        let uid = user_id();
        let _ = run(
            "launchctl",
            &[s("bootout"), format!("gui/{uid}/{LAUNCHD_LABEL}")],
        );
        run(
            "launchctl",
            &[
                s("bootstrap"),
                format!("gui/{uid}"),
                plist.display().to_string(),
            ],
        )?;
        done.push(format!(
            "daemon: launchd agent {LAUNCHD_LABEL} loaded ({})",
            plist.display()
        ));

        let app = paths.url_app();
        let script = paths.state_dir.join("url-handler.applescript");
        write(&script, &url_handler_applescript(&paths.exe))?;
        let _ = std::fs::remove_dir_all(&app);
        std::fs::create_dir_all(app.parent().unwrap_or(Path::new(".")))?;
        run(
            "osacompile",
            &[
                s("-o"),
                app.display().to_string(),
                script.display().to_string(),
            ],
        )?;
        let info = app.join("Contents/Info.plist");
        run(
            "plutil",
            &[
                s("-insert"),
                s("CFBundleURLTypes"),
                s("-json"),
                format!(r#"[{{"CFBundleURLName":"Kintsu","CFBundleURLSchemes":["{SCHEME}"]}}]"#),
                info.display().to_string(),
            ],
        )?;
        let _ = run(
            "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister",
            &[s("-f"), app.display().to_string()],
        );
        done.push(format!(
            "clicks: {} handles {SCHEME}:// links",
            app.display()
        ));
    } else {
        let unit = paths.systemd_unit();
        write(&unit, &systemd_unit(&paths.exe))?;
        run("systemctl", &[s("--user"), s("daemon-reload")])?;
        run(
            "systemctl",
            &[s("--user"), s("enable"), s("--now"), s("kintsu.service")],
        )?;
        done.push(format!(
            "daemon: systemd user unit kintsu.service enabled ({})",
            unit.display()
        ));

        let entry = paths.desktop_entry();
        write(&entry, &desktop_entry(&paths.exe))?;
        run(
            "xdg-mime",
            &[
                s("default"),
                s("kintsu.desktop"),
                format!("x-scheme-handler/{SCHEME}"),
            ],
        )?;
        let _ = run(
            "update-desktop-database",
            &[entry
                .parent()
                .unwrap_or(Path::new("."))
                .display()
                .to_string()],
        );
        done.push(format!(
            "clicks: {} handles {SCHEME}:// links",
            entry.display()
        ));
    }
    Ok(done)
}

/// Removes what `install` put in place.
pub fn uninstall(paths: &ServicePaths, macos: bool, run: Runner<'_>) -> io::Result<Vec<String>> {
    let mut done = Vec::new();
    let s = |v: &str| v.to_string();
    if macos {
        let _ = run(
            "launchctl",
            &[s("bootout"), format!("gui/{}/{LAUNCHD_LABEL}", user_id())],
        );
        if std::fs::remove_file(paths.launch_agent()).is_ok() {
            done.push("daemon: launchd agent removed".into());
        }
        if std::fs::remove_dir_all(paths.url_app()).is_ok() {
            done.push("clicks: Kintsu.app removed".into());
        }
    } else {
        let _ = run(
            "systemctl",
            &[s("--user"), s("disable"), s("--now"), s("kintsu.service")],
        );
        if std::fs::remove_file(paths.systemd_unit()).is_ok() {
            done.push("daemon: systemd unit removed".into());
        }
        if std::fs::remove_file(paths.desktop_entry()).is_ok() {
            done.push("clicks: desktop entry removed".into());
        }
    }
    Ok(done)
}

fn write(path: &Path, content: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)
}

fn user_id() -> u32 {
    crate::adapters::gateways::unix::uid()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(home: &Path) -> ServicePaths {
        ServicePaths {
            home: home.to_path_buf(),
            state_dir: home.join("state"),
            exe: PathBuf::from("/opt/kintsu/bin/kintsu"),
        }
    }

    #[test]
    fn the_files_name_the_binary_the_scheme_and_the_daemon_command() {
        let exe = Path::new("/opt/kintsu/bin/kintsu");
        let plist = launch_agent_plist(exe, Path::new("/s/daemon.log"));
        assert!(plist.contains(
            "<string>/opt/kintsu/bin/kintsu</string><string>daemon</string><string>run</string>"
        ));
        assert!(plist.contains("<key>KeepAlive</key><true/>"));
        assert!(systemd_unit(exe).contains("ExecStart=/opt/kintsu/bin/kintsu daemon run"));
        assert!(url_handler_applescript(exe).contains("& \" open \" & quoted form of this_url"));
        let entry = desktop_entry(exe);
        assert!(
            entry.contains("Exec=/opt/kintsu/bin/kintsu open %u")
                && entry.contains("MimeType=x-scheme-handler/kintsu;")
        );
    }

    #[test]
    fn install_writes_the_files_and_runs_the_registrations_on_each_system() {
        let home = std::env::temp_dir().join(format!("kintsu-service-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let p = paths(&home);
        let mut ran: Vec<String> = Vec::new();
        let mut record = |program: &str, args: &[String]| {
            ran.push(format!("{program} {}", args.join(" ")));
            Ok(())
        };
        let done = install(&p, false, &mut record).unwrap();
        assert!(p.systemd_unit().is_file() && p.desktop_entry().is_file());
        assert!(
            ran.iter()
                .any(|r| r == "systemctl --user enable --now kintsu.service"),
            "{ran:?}"
        );
        assert!(
            ran.iter()
                .any(|r| r == "xdg-mime default kintsu.desktop x-scheme-handler/kintsu"),
            "{ran:?}"
        );
        assert_eq!(done.len(), 2);
        let mut ran_mac: Vec<String> = Vec::new();
        let mut record_mac = |program: &str, args: &[String]| {
            ran_mac.push(format!("{program} {}", args.join(" ")));
            Ok(())
        };
        let done = install(&p, true, &mut record_mac).unwrap();
        assert!(p.launch_agent().is_file());
        assert!(
            ran_mac
                .iter()
                .any(|r| r.starts_with("launchctl bootstrap gui/")),
            "{ran_mac:?}"
        );
        assert!(
            ran_mac.iter().any(|r| r.starts_with("osacompile -o")),
            "{ran_mac:?}"
        );
        assert!(
            ran_mac.iter().any(|r| r.contains("CFBundleURLSchemes")),
            "{ran_mac:?}"
        );
        assert_eq!(done.len(), 2);
        let mut noop = |_: &str, _: &[String]| Ok(());
        let undone = uninstall(&p, false, &mut noop).unwrap();
        assert_eq!(undone.len(), 2);
        assert!(!p.systemd_unit().exists());
        let _ = std::fs::remove_dir_all(&home);
    }
}

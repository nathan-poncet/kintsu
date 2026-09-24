//! Sessions, last cases and the ignore list as small JSON files under the
//! state directory. Written atomically; unreadable files count as absent
//! only when they are missing, never when they are corrupt.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::entities::{
    CaseId, CommandLine, CommandOutcome, Duration, ExitStatus, FailureCase, IgnoreEntry,
    IgnoreScope, IgnoreTarget, Session, SessionId, Shell, Timestamp,
};
use crate::use_cases::ports::{
    CaseStore, CaseStoreError, IgnoreStore, IgnoreStoreError, RegistryError, SessionRegistry,
};

pub struct JsonState {
    dir: PathBuf,
}

impl JsonState {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn session_file(&self, id: &SessionId) -> PathBuf {
        self.dir
            .join("sessions")
            .join(format!("{}.json", safe_name(id.as_str())))
    }

    fn session_case_file(&self, id: &SessionId) -> PathBuf {
        self.dir
            .join("cases")
            .join(format!("session-{}.json", safe_name(id.as_str())))
    }

    fn last_case_file(&self) -> PathBuf {
        self.dir.join("cases").join("last.json")
    }

    fn ignore_file(&self) -> PathBuf {
        self.dir.join("ignore.json")
    }

    fn read<T: DeserializeOwned>(&self, path: &Path) -> Result<Option<T>, String> {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    fn write<T: Serialize>(&self, path: &Path, value: &T) -> Result<(), String> {
        let parent = path.parent().ok_or("no parent directory")?;
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
        let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// Only what a file name can hold: the rest becomes its hex.
fn safe_name(id: &str) -> String {
    if !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        id.to_string()
    } else {
        id.bytes().map(|b| format!("{b:02x}")).collect()
    }
}

#[derive(Serialize, Deserialize)]
struct OutcomeDto {
    command: String,
    status: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    duration_ms: Option<u64>,
}

impl OutcomeDto {
    fn from(o: &CommandOutcome) -> Self {
        Self {
            command: o.command().as_str().to_string(),
            status: o.status().code(),
            duration_ms: o.duration().map(|d| d.as_millis()),
        }
    }

    fn into_outcome(self) -> Option<CommandOutcome> {
        let mut outcome = CommandOutcome::new(
            CommandLine::new(self.command).ok()?,
            ExitStatus::new(self.status),
        );
        if let Some(ms) = self.duration_ms {
            outcome = outcome.lasting(Duration::from_millis(ms));
        }
        Some(outcome)
    }
}

#[derive(Serialize, Deserialize)]
struct SessionDto {
    id: String,
    shell: Option<String>,
    recent: Vec<OutcomeDto>,
}

impl SessionRegistry for JsonState {
    fn load(&self, id: &SessionId) -> Result<Option<Session>, RegistryError> {
        let dto: Option<SessionDto> = self
            .read(&self.session_file(id))
            .map_err(RegistryError::Unavailable)?;
        Ok(dto.map(|d| {
            let shell = d.shell.as_deref().and_then(Shell::from_name);
            Session::with_recent(
                SessionId::new(d.id),
                shell,
                d.recent
                    .into_iter()
                    .filter_map(OutcomeDto::into_outcome)
                    .collect(),
            )
        }))
    }

    fn save(&self, session: &Session) -> Result<(), RegistryError> {
        let dto = SessionDto {
            id: session.id().as_str().to_string(),
            shell: session.shell().map(|s| s.name().to_string()),
            recent: session.recent().iter().map(OutcomeDto::from).collect(),
        };
        self.write(&self.session_file(session.id()), &dto)
            .map_err(RegistryError::Unavailable)
    }
}

#[derive(Serialize, Deserialize)]
struct CaseDto {
    id: String,
    at_ms: u64,
    outcome: OutcomeDto,
    cwd: Option<String>,
    session: Option<String>,
    recent: Vec<String>,
    output: Option<String>,
}

impl CaseDto {
    fn from(c: &FailureCase) -> Self {
        Self {
            id: c.id().as_str().to_string(),
            at_ms: c.at().as_millis(),
            outcome: OutcomeDto::from(c.outcome()),
            cwd: c.cwd().map(String::from),
            session: c.session().map(|s| s.as_str().to_string()),
            recent: c.recent().iter().map(|l| l.as_str().to_string()).collect(),
            output: c.output().map(String::from),
        }
    }

    fn into_case(self) -> Option<FailureCase> {
        let mut case = FailureCase::new(
            CaseId::new(self.id),
            Timestamp::from_millis(self.at_ms),
            self.outcome.into_outcome()?,
            self.cwd,
        )
        .with_session(self.session.map(SessionId::new))
        .with_recent(
            self.recent
                .into_iter()
                .filter_map(|l| CommandLine::new(l).ok())
                .collect(),
        );
        if let Some(output) = self.output {
            case = case.with_output(output);
        }
        Some(case)
    }
}

impl CaseStore for JsonState {
    fn save(&self, case: &FailureCase) -> Result<(), CaseStoreError> {
        let dto = CaseDto::from(case);
        if let Some(session) = case.session() {
            self.write(&self.session_case_file(session), &dto)
                .map_err(CaseStoreError::Unavailable)?;
        }
        self.write(&self.last_case_file(), &dto)
            .map_err(CaseStoreError::Unavailable)
    }

    fn last(&self, session: Option<&SessionId>) -> Result<Option<FailureCase>, CaseStoreError> {
        let path = match session {
            Some(id) => self.session_case_file(id),
            None => self.last_case_file(),
        };
        let dto: Option<CaseDto> = self.read(&path).map_err(CaseStoreError::Unavailable)?;
        Ok(dto.and_then(CaseDto::into_case))
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
enum TargetDto {
    Program(String),
    Command(String),
    Everything,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
enum ScopeDto {
    Everywhere,
    Directory(String),
    Session(String),
    Until(u64),
}

#[derive(Serialize, Deserialize)]
struct IgnoreDto {
    target: TargetDto,
    scope: ScopeDto,
}

impl IgnoreDto {
    fn from(e: &IgnoreEntry) -> Self {
        Self {
            target: match e.target() {
                IgnoreTarget::Program(p) => TargetDto::Program(p.clone()),
                IgnoreTarget::Command(c) => TargetDto::Command(c.clone()),
                IgnoreTarget::Everything => TargetDto::Everything,
            },
            scope: match e.scope() {
                IgnoreScope::Everywhere => ScopeDto::Everywhere,
                IgnoreScope::Directory(d) => ScopeDto::Directory(d.clone()),
                IgnoreScope::Session(s) => ScopeDto::Session(s.as_str().to_string()),
                IgnoreScope::Until(t) => ScopeDto::Until(t.as_millis()),
            },
        }
    }

    fn into_entry(self) -> IgnoreEntry {
        IgnoreEntry::new(
            match self.target {
                TargetDto::Program(p) => IgnoreTarget::Program(p),
                TargetDto::Command(c) => IgnoreTarget::Command(c),
                TargetDto::Everything => IgnoreTarget::Everything,
            },
            match self.scope {
                ScopeDto::Everywhere => IgnoreScope::Everywhere,
                ScopeDto::Directory(d) => IgnoreScope::Directory(d),
                ScopeDto::Session(s) => IgnoreScope::Session(SessionId::new(s)),
                ScopeDto::Until(t) => IgnoreScope::Until(Timestamp::from_millis(t)),
            },
        )
    }
}

impl IgnoreStore for JsonState {
    fn entries(&self) -> Result<Vec<IgnoreEntry>, IgnoreStoreError> {
        let dtos: Option<Vec<IgnoreDto>> = self
            .read(&self.ignore_file())
            .map_err(IgnoreStoreError::Unavailable)?;
        Ok(dtos
            .unwrap_or_default()
            .into_iter()
            .map(IgnoreDto::into_entry)
            .collect())
    }

    fn replace(&self, entries: &[IgnoreEntry]) -> Result<(), IgnoreStoreError> {
        let dtos: Vec<IgnoreDto> = entries.iter().map(IgnoreDto::from).collect();
        self.write(&self.ignore_file(), &dtos)
            .map_err(IgnoreStoreError::Unavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kintsu-state-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn outcome(text: &str, code: i32) -> CommandOutcome {
        CommandOutcome::new(CommandLine::new(text).unwrap(), ExitStatus::new(code))
    }

    #[test]
    fn sessions_round_trip_with_their_shell_and_history() {
        let dir = scratch("sessions");
        let state = JsonState::new(&dir);
        let id = SessionId::new("42");
        assert_eq!(state.load(&id).unwrap(), None);
        let mut session = Session::new(id.clone(), Some(Shell::Fish));
        session.remember(outcome("ls", 0));
        session.remember(outcome("make", 2).lasting(Duration::from_secs(3)));
        SessionRegistry::save(&state, &session).unwrap();
        assert_eq!(state.load(&id).unwrap(), Some(session));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_last_case_is_kept_per_session_and_overall() {
        let dir = scratch("cases");
        let state = JsonState::new(&dir);
        let a = FailureCase::new(
            CaseId::new("a"),
            Timestamp::from_millis(1),
            outcome("make", 2),
            Some("/w".into()),
        )
        .with_session(Some(SessionId::new("s1")))
        .with_recent(vec![CommandLine::new("ls").unwrap()])
        .with_output("boom".into());
        let b = FailureCase::new(
            CaseId::new("b"),
            Timestamp::from_millis(2),
            outcome("cargo", 101),
            None,
        )
        .with_session(Some(SessionId::new("s2/odd id")));
        CaseStore::save(&state, &a).unwrap();
        CaseStore::save(&state, &b).unwrap();
        assert_eq!(state.last(Some(&SessionId::new("s1"))).unwrap(), Some(a));
        assert_eq!(
            state.last(Some(&SessionId::new("s2/odd id"))).unwrap(),
            Some(b.clone())
        );
        assert_eq!(state.last(None).unwrap(), Some(b));
        assert_eq!(state.last(Some(&SessionId::new("s3"))).unwrap(), None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_ignore_list_round_trips_and_a_corrupt_file_is_an_error() {
        let dir = scratch("ignore");
        let state = JsonState::new(&dir);
        assert!(state.entries().unwrap().is_empty());
        let entries = vec![
            IgnoreEntry::new(
                IgnoreTarget::Program("make".into()),
                IgnoreScope::Directory("/w".into()),
            ),
            IgnoreEntry::new(
                IgnoreTarget::Command("npm test".into()),
                IgnoreScope::Everywhere,
            ),
            IgnoreEntry::new(
                IgnoreTarget::Program("x".into()),
                IgnoreScope::Session(SessionId::new("7")),
            ),
            IgnoreEntry::mute_until(Timestamp::from_millis(99)),
        ];
        state.replace(&entries).unwrap();
        assert_eq!(state.entries().unwrap(), entries);
        std::fs::write(state.ignore_file(), b"{not json").unwrap();
        assert!(matches!(
            state.entries(),
            Err(IgnoreStoreError::Unavailable(_))
        ));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn odd_session_ids_become_safe_file_names() {
        assert_eq!(safe_name("42"), "42");
        assert_eq!(safe_name("tty-1_a"), "tty-1_a");
        assert_eq!(safe_name("../x"), "2e2e2f78");
        assert_eq!(safe_name(""), "");
    }
}

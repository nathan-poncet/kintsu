//! Sessions, last cases and the ignore list as small JSON files under the
//! state directory: how v0.1 and v0.2 kept state. Written atomically;
//! unreadable files count as absent only when they are missing, never when
//! they are corrupt. Since SQLite took over, this is what the migration
//! reads, and the documents SQLite keeps use the same shapes.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::entities::{
    CaseId, CommandLine, CommandOutcome, Confidence, Duration, ExitStatus, Explanation,
    FailureCase, Fix, FixSource, IgnoreEntry, IgnoreScope, IgnoreTarget, Session, SessionId, Shell,
    Timestamp,
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

impl JsonState {
    /// Whether any JSON state is there to migrate.
    pub fn has_files(&self) -> bool {
        !self.files().is_empty()
    }

    /// Every session on file, in no particular order; a file that cannot
    /// be read is left out, a migration must not fail on one.
    pub fn sessions(&self) -> Vec<Session> {
        let mut sessions = Vec::new();
        for path in json_files(&self.dir.join("sessions")) {
            let dto: Option<SessionDto> = self.read(&path).ok().flatten();
            if let Some(d) = dto {
                sessions.push(Session::with_recent(
                    SessionId::new(d.id),
                    d.shell.as_deref().and_then(Shell::from_name),
                    d.recent
                        .into_iter()
                        .filter_map(OutcomeDto::into_outcome)
                        .collect(),
                ));
            }
        }
        sessions
    }

    /// Every case on file, oldest first, the last one overall last.
    pub fn cases(&self) -> Vec<FailureCase> {
        let mut cases = Vec::new();
        for path in json_files(&self.dir.join("cases")) {
            if path == self.last_case_file() {
                continue;
            }
            let dto: Option<CaseDto> = self.read(&path).ok().flatten();
            cases.extend(dto.and_then(CaseDto::into_case));
        }
        cases.sort_by_key(|c| c.at().as_millis());
        let last: Option<CaseDto> = self.read(&self.last_case_file()).ok().flatten();
        cases.extend(last.and_then(CaseDto::into_case));
        cases
    }

    /// The files the state is made of, for moving them aside once read.
    pub fn files(&self) -> Vec<PathBuf> {
        let mut files = json_files(&self.dir.join("sessions"));
        files.extend(json_files(&self.dir.join("cases")));
        if self.ignore_file().is_file() {
            files.push(self.ignore_file());
        }
        files
    }
}

/// The `.json` files of a directory; the hooks' marker files and anything
/// already moved aside are not among them.
fn json_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    files
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
pub(super) struct OutcomeDto {
    command: String,
    status: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pipestatus: Vec<i32>,
}

impl OutcomeDto {
    pub(super) fn from(o: &CommandOutcome) -> Self {
        Self {
            command: o.command().as_str().to_string(),
            status: o.status().code(),
            duration_ms: o.duration().map(|d| d.as_millis()),
            pipestatus: o.pipestatus().iter().map(|s| s.code()).collect(),
        }
    }

    pub(super) fn into_outcome(self) -> Option<CommandOutcome> {
        let mut outcome = CommandOutcome::new(
            CommandLine::new(self.command).ok()?,
            ExitStatus::new(self.status),
        );
        if let Some(ms) = self.duration_ms {
            outcome = outcome.lasting(Duration::from_millis(ms));
        }
        Some(outcome.in_pipeline(self.pipestatus.into_iter().map(ExitStatus::new).collect()))
    }
}

#[derive(Serialize, Deserialize)]
pub(super) struct SessionDto {
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
pub(super) struct FixDto {
    command: String,
    confidence: f32,
    source_kind: String,
    source_name: String,
    rationale: String,
}

impl FixDto {
    fn from(f: &Fix) -> Self {
        let (source_kind, source_name) = match f.source() {
            FixSource::Rule(name) => ("rule", name.clone()),
            FixSource::Model(name) => ("model", name.clone()),
        };
        Self {
            command: f.command().as_str().to_string(),
            confidence: f.confidence().value(),
            source_kind: source_kind.to_string(),
            source_name,
            rationale: f.rationale().to_string(),
        }
    }

    fn into_fix(self) -> Option<Fix> {
        let source = match self.source_kind.as_str() {
            "rule" => FixSource::Rule(self.source_name),
            _ => FixSource::Model(self.source_name),
        };
        Some(Fix::new(
            CommandLine::new(self.command).ok()?,
            Confidence::new(self.confidence),
            source,
            self.rationale,
        ))
    }
}

#[derive(Serialize, Deserialize)]
pub(super) struct ExplanationDto {
    model: String,
    text: String,
}

#[derive(Serialize, Deserialize)]
pub(super) struct CaseDto {
    id: String,
    at_ms: u64,
    outcome: OutcomeDto,
    cwd: Option<String>,
    session: Option<String>,
    recent: Vec<String>,
    output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    proposal: Option<FixDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    explanation: Option<ExplanationDto>,
}

impl CaseDto {
    pub(super) fn from(c: &FailureCase) -> Self {
        Self {
            id: c.id().as_str().to_string(),
            at_ms: c.at().as_millis(),
            outcome: OutcomeDto::from(c.outcome()),
            cwd: c.cwd().map(String::from),
            session: c.session().map(|s| s.as_str().to_string()),
            recent: c.recent().iter().map(|l| l.as_str().to_string()).collect(),
            output: c.output().map(String::from),
            proposal: c.proposal().map(FixDto::from),
            explanation: c.explanation().map(|e| ExplanationDto {
                model: e.model().to_string(),
                text: e.text().to_string(),
            }),
        }
    }

    pub(super) fn into_case(self) -> Option<FailureCase> {
        let mut case = FailureCase::new(
            CaseId::new(self.id),
            Timestamp::from_millis(self.at_ms),
            self.outcome.into_outcome()?,
            self.cwd,
        )
        .with_session(self.session.map(SessionId::new))
        .with_proposal(self.proposal.and_then(FixDto::into_fix))
        .with_recent(
            self.recent
                .into_iter()
                .filter_map(|l| CommandLine::new(l).ok())
                .collect(),
        );
        if let Some(output) = self.output {
            case = case.with_output(output);
        }
        if let Some(e) = self.explanation {
            case = case.with_explanation(Explanation::new(e.model, e.text));
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
pub(super) enum TargetDto {
    Program(String),
    Command(String),
    Everything,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub(super) enum ScopeDto {
    Everywhere,
    Directory(String),
    Session(String),
    Until(u64),
}

#[derive(Serialize, Deserialize)]
pub(super) struct IgnoreDto {
    pub(super) target: TargetDto,
    pub(super) scope: ScopeDto,
}

impl IgnoreDto {
    pub(super) fn from(e: &IgnoreEntry) -> Self {
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

    pub(super) fn into_entry(self) -> IgnoreEntry {
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
    use crate::use_cases::testing::storage_contract;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kintsu-state-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn the_json_files_honour_the_storage_contract() {
        let dir = scratch("contract");
        storage_contract::everything(&JsonState::new(&dir));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_corrupt_file_is_an_error_not_an_absence() {
        let dir = scratch("corrupt");
        let state = JsonState::new(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(state.ignore_file(), b"{not json").unwrap();
        assert!(matches!(
            state.entries(),
            Err(IgnoreStoreError::Unavailable(_))
        ));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_whole_state_can_be_read_back_for_a_migration() {
        let dir = scratch("readback");
        let state = JsonState::new(&dir);
        assert!(!state.has_files());
        storage_contract::everything(&state);
        std::fs::write(
            dir.join("sessions").join("42.ghost"),
            b"a marker, not a session",
        )
        .unwrap();
        let sessions = state.sessions();
        assert_eq!(
            sessions.len(),
            2,
            "42 and p; the marker file is not a session"
        );
        std::fs::write(dir.join("cases").join("session-broken.json"), b"{not json").unwrap();
        let cases = state.cases();
        assert_eq!(cases.len(), 3, "one per session, then the last overall");
        assert_eq!(cases.last().unwrap().id().as_str(), "a");
        assert!(
            state
                .files()
                .iter()
                .all(|f| f.extension().unwrap() == "json")
        );
        assert_eq!(
            state.files().len(),
            7,
            "two sessions, two per-session cases, the broken one, last.json, the ignore list"
        );
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

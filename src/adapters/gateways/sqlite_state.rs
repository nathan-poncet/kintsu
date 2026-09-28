//! Sessions, cases and the ignore list in one SQLite file under the state
//! directory, `kintsu.db`: what replaced the JSON files. The first open
//! creates the schema and, when JSON state is there, imports it and moves
//! those files aside. The only module that speaks SQL.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use super::json_state::{CaseDto, IgnoreDto, JsonState, OutcomeDto};
use crate::entities::{FailureCase, IgnoreEntry, Session, SessionId, Shell};
use crate::use_cases::ports::{
    CaseStore, CaseStoreError, IgnoreStore, IgnoreStoreError, RegistryError, SessionRegistry,
};

/// Cases older than the newest this many are forgotten, the last case of a
/// shell that failed long ago with them.
const KEPT_CASES: i64 = 5000;

/// How long a writer waits for another process's transaction: the daemon
/// and a hook client may open the file at the same moment.
const BUSY_TIMEOUT: Duration = Duration::from_millis(250);

const SCHEMA: &str = "
CREATE TABLE sessions (id TEXT PRIMARY KEY, shell TEXT, recent TEXT NOT NULL);
CREATE TABLE cases (
    id TEXT PRIMARY KEY,
    session TEXT,
    at_ms INTEGER NOT NULL,
    seq INTEGER NOT NULL,
    command TEXT NOT NULL,
    status INTEGER NOT NULL,
    cwd TEXT,
    document TEXT NOT NULL
);
CREATE INDEX cases_by_session ON cases (session, seq);
CREATE INDEX cases_by_seq ON cases (seq);
CREATE TABLE ignores (position INTEGER PRIMARY KEY, target TEXT NOT NULL, scope TEXT NOT NULL);
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
";

const MIGRATION_NOTE: &str = "migrated_from_json";

/// What `kintsu doctor` says about the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreSummary {
    pub path: PathBuf,
    pub cases: u64,
    pub sessions: u64,
    /// What the first open imported from the JSON files, when it did.
    pub migrated: Option<String>,
}

pub struct SqliteState {
    dir: PathBuf,
    connection: Mutex<Option<Connection>>,
}

impl SqliteState {
    /// Over the state directory; nothing is opened until the first use.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            connection: Mutex::new(None),
        }
    }

    /// The database file.
    pub fn path(&self) -> PathBuf {
        self.dir.join("kintsu.db")
    }

    /// Counts and the migration note, for the doctor's report.
    pub fn summary(&self) -> Result<StoreSummary, String> {
        self.with(|c| {
            let count = |table: &str| -> rusqlite::Result<u64> {
                c.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            };
            Ok(StoreSummary {
                path: self.path(),
                cases: count("cases")?,
                sessions: count("sessions")?,
                migrated: c
                    .query_row(
                        "SELECT value FROM meta WHERE key = ?1",
                        [MIGRATION_NOTE],
                        |r| r.get(0),
                    )
                    .optional()?,
            })
        })
    }

    fn with<R>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<R>) -> Result<R, String> {
        let mut guard = self.connection.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            *guard = Some(self.open()?);
        }
        let connection = guard.as_ref().expect("opened just above");
        f(connection).map_err(|e| format!("{}: {e}", self.path().display()))
    }

    fn open(&self) -> Result<Connection, String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| format!("{}: {e}", self.dir.display()))?;
        let describe = |e: rusqlite::Error| format!("{}: {e}", self.path().display());
        let mut connection = Connection::open(self.path()).map_err(describe)?;
        connection.busy_timeout(BUSY_TIMEOUT).map_err(describe)?;
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(describe)?;
        connection
            .pragma_update(None, "synchronous", "NORMAL")
            .map_err(describe)?;
        self.prepare(&mut connection).map_err(describe)?;
        Ok(connection)
    }

    /// The schema, once; the JSON import with it, so a second process that
    /// opens the file at the same moment waits and finds both done.
    fn prepare(&self, connection: &mut Connection) -> rusqlite::Result<()> {
        let version: i64 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version != 0 {
            return Ok(());
        }
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        // Read again under the lock: another process may have got there first.
        let version: i64 = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version == 0 {
            tx.execute_batch(SCHEMA)?;
            if let Some(note) = self.import_json(&tx)? {
                tx.execute(
                    "INSERT INTO meta (key, value) VALUES (?1, ?2)",
                    params![MIGRATION_NOTE, note],
                )?;
            }
            tx.pragma_update(None, "user_version", 1)?;
        }
        tx.commit()
    }

    /// The JSON files of the versions before, read once and moved aside as
    /// `*.migrated`. A file that cannot be read is left out, not fatal: the
    /// store must open.
    fn import_json(&self, connection: &Connection) -> rusqlite::Result<Option<String>> {
        let json = JsonState::new(&self.dir);
        if !json.has_files() {
            return Ok(None);
        }
        let sessions = json.sessions();
        for session in &sessions {
            save_session(connection, session)?;
        }
        let cases = json.cases();
        for case in &cases {
            write_case(connection, case)?;
        }
        let ignores = json.entries().unwrap_or_default();
        write_ignores(connection, &ignores)?;
        for file in json.files() {
            let aside = file.with_extension("json.migrated");
            let _ = std::fs::rename(&file, aside);
        }
        Ok(Some(format!(
            "{} sessions, {} cases and {} ignore entries imported from the JSON files",
            sessions.len(),
            cases.len(),
            ignores.len()
        )))
    }
}

fn save_session(connection: &Connection, session: &Session) -> rusqlite::Result<()> {
    let recent: Vec<OutcomeDto> = session.recent().iter().map(OutcomeDto::from).collect();
    connection.execute(
        "INSERT INTO sessions (id, shell, recent) VALUES (?1, ?2, ?3)
         ON CONFLICT(id) DO UPDATE SET shell = excluded.shell, recent = excluded.recent",
        params![
            session.id().as_str(),
            session.shell().map(Shell::name),
            json(&recent)?
        ],
    )?;
    Ok(())
}

/// Saving makes the case the newest, of its session and overall, whether it
/// is new or saved again with more in it.
fn save_case(connection: &Connection, case: &FailureCase) -> rusqlite::Result<()> {
    let tx = connection.unchecked_transaction()?;
    write_case(&tx, case)?;
    tx.commit()
}

fn write_case(connection: &Connection, case: &FailureCase) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT INTO cases (id, session, at_ms, seq, command, status, cwd, document)
         VALUES (?1, ?2, ?3, (SELECT COALESCE(MAX(seq), 0) + 1 FROM cases), ?4, ?5, ?6, ?7)
         ON CONFLICT(id) DO UPDATE SET
             session = excluded.session, at_ms = excluded.at_ms, seq = excluded.seq,
             command = excluded.command, status = excluded.status, cwd = excluded.cwd,
             document = excluded.document",
        params![
            case.id().as_str(),
            case.session().map(SessionId::as_str),
            case.at().as_millis() as i64,
            case.outcome().command().as_str(),
            case.outcome().status().code(),
            case.cwd(),
            json(&CaseDto::from(case))?
        ],
    )?;
    connection.execute(
        "DELETE FROM cases WHERE seq <= (SELECT MAX(seq) FROM cases) - ?1",
        [KEPT_CASES],
    )?;
    Ok(())
}

fn replace_ignores(connection: &Connection, entries: &[IgnoreEntry]) -> rusqlite::Result<()> {
    let tx = connection.unchecked_transaction()?;
    write_ignores(&tx, entries)?;
    tx.commit()
}

/// The list, replaced whole, inside whatever transaction the caller holds.
fn write_ignores(connection: &Connection, entries: &[IgnoreEntry]) -> rusqlite::Result<()> {
    connection.execute("DELETE FROM ignores", [])?;
    for (position, entry) in entries.iter().enumerate() {
        let dto = IgnoreDto::from(entry);
        connection.execute(
            "INSERT INTO ignores (position, target, scope) VALUES (?1, ?2, ?3)",
            params![position as i64, json(&dto.target)?, json(&dto.scope)?],
        )?;
    }
    Ok(())
}

fn json<T: serde::Serialize>(value: &T) -> rusqlite::Result<String> {
    serde_json::to_string(value).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
}

fn parsed<T: serde::de::DeserializeOwned>(text: String) -> rusqlite::Result<T> {
    serde_json::from_str(&text).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

impl SessionRegistry for SqliteState {
    fn load(&self, id: &SessionId) -> Result<Option<Session>, RegistryError> {
        self.with(|c| {
            let row: Option<(Option<String>, String)> = c
                .query_row(
                    "SELECT shell, recent FROM sessions WHERE id = ?1",
                    [id.as_str()],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            let Some((shell, recent)) = row else {
                return Ok(None);
            };
            let recent: Vec<OutcomeDto> = parsed(recent)?;
            Ok(Some(Session::with_recent(
                id.clone(),
                shell.as_deref().and_then(Shell::from_name),
                recent
                    .into_iter()
                    .filter_map(OutcomeDto::into_outcome)
                    .collect(),
            )))
        })
        .map_err(RegistryError::Unavailable)
    }

    fn save(&self, session: &Session) -> Result<(), RegistryError> {
        self.with(|c| save_session(c, session))
            .map_err(RegistryError::Unavailable)
    }
}

impl CaseStore for SqliteState {
    fn save(&self, case: &FailureCase) -> Result<(), CaseStoreError> {
        self.with(|c| save_case(c, case))
            .map_err(CaseStoreError::Unavailable)
    }

    fn last(&self, session: Option<&SessionId>) -> Result<Option<FailureCase>, CaseStoreError> {
        self.with(|c| {
            let document: Option<String> = match session {
                Some(id) => c
                    .query_row(
                        "SELECT document FROM cases WHERE session = ?1 ORDER BY seq DESC LIMIT 1",
                        [id.as_str()],
                        |r| r.get(0),
                    )
                    .optional()?,
                None => c
                    .query_row(
                        "SELECT document FROM cases ORDER BY seq DESC LIMIT 1",
                        [],
                        |r| r.get(0),
                    )
                    .optional()?,
            };
            match document {
                Some(text) => Ok(parsed::<CaseDto>(text)?.into_case()),
                None => Ok(None),
            }
        })
        .map_err(CaseStoreError::Unavailable)
    }
}

impl IgnoreStore for SqliteState {
    fn entries(&self) -> Result<Vec<IgnoreEntry>, IgnoreStoreError> {
        self.with(|c| {
            let mut statement = c.prepare("SELECT target, scope FROM ignores ORDER BY position")?;
            let rows = statement
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            let mut entries = Vec::new();
            for row in rows {
                let (target, scope) = row?;
                let dto = IgnoreDto {
                    target: parsed(target)?,
                    scope: parsed(scope)?,
                };
                entries.push(dto.into_entry());
            }
            Ok(entries)
        })
        .map_err(IgnoreStoreError::Unavailable)
    }

    fn replace(&self, entries: &[IgnoreEntry]) -> Result<(), IgnoreStoreError> {
        self.with(|c| replace_ignores(c, entries))
            .map_err(IgnoreStoreError::Unavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{
        CaseId, CommandLine, CommandOutcome, ExitStatus, IgnoreScope, IgnoreTarget, Timestamp,
    };
    use crate::use_cases::testing::storage_contract;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kintsu-sqlite-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn case(id: &str, session: &str, at: u64) -> FailureCase {
        FailureCase::new(
            CaseId::new(id),
            Timestamp::from_millis(at),
            CommandOutcome::new(CommandLine::new("make").unwrap(), ExitStatus::new(2)),
            None,
        )
        .with_session(Some(SessionId::new(session)))
    }

    #[test]
    fn sqlite_honours_the_storage_contract() {
        let dir = scratch("contract");
        storage_contract::everything(&SqliteState::new(&dir));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_second_opening_sees_what_the_first_wrote() {
        let dir = scratch("reopen");
        let session = Session::new(SessionId::new("42"), Some(Shell::Zsh));
        SessionRegistry::save(&SqliteState::new(&dir), &session).unwrap();
        let again = SqliteState::new(&dir);
        assert_eq!(again.load(&SessionId::new("42")).unwrap(), Some(session));
        let summary = again.summary().unwrap();
        assert_eq!(
            (summary.sessions, summary.cases, summary.migrated),
            (1, 0, None)
        );
        assert_eq!(summary.path, dir.join("kintsu.db"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_corrupt_database_file_is_an_error_not_an_absence() {
        let dir = scratch("corrupt");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("kintsu.db"), b"this is not a database at all").unwrap();
        let state = SqliteState::new(&dir);
        assert!(matches!(
            state.last(None),
            Err(CaseStoreError::Unavailable(_))
        ));
        assert!(matches!(
            state.load(&SessionId::new("42")),
            Err(RegistryError::Unavailable(_))
        ));
        assert!(matches!(
            state.entries(),
            Err(IgnoreStoreError::Unavailable(_))
        ));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_json_state_is_imported_once_and_its_files_moved_aside() {
        let dir = scratch("migration");
        let json = JsonState::new(&dir);
        storage_contract::everything(&json);
        let muted = vec![IgnoreEntry::new(
            IgnoreTarget::Program("make".into()),
            IgnoreScope::Everywhere,
        )];
        json.replace(&muted).unwrap();
        std::fs::write(dir.join("sessions").join("42.ghost"), b"a marker").unwrap();

        let state = SqliteState::new(&dir);
        let session = state.load(&SessionId::new("42")).unwrap().unwrap();
        assert_eq!(session.shell(), Some(Shell::Fish));
        assert_eq!(session.recent().len(), 3);
        assert_eq!(
            state
                .last(Some(&SessionId::new("s1")))
                .unwrap()
                .unwrap()
                .id()
                .as_str(),
            "a"
        );
        assert_eq!(
            state.last(None).unwrap().unwrap().id().as_str(),
            "a",
            "last.json decides the last overall"
        );
        assert_eq!(state.entries().unwrap(), muted);
        let summary = state.summary().unwrap();
        assert_eq!(
            summary.migrated.as_deref(),
            Some("2 sessions, 3 cases and 1 ignore entries imported from the JSON files")
        );
        assert!(!json.has_files(), "every JSON file was moved aside");
        assert!(dir.join("ignore.json.migrated").is_file());
        assert!(
            dir.join("sessions").join("42.ghost").is_file(),
            "the hooks' markers are not state"
        );

        // A JSON file appearing later is not imported: the migration ran once.
        drop(state);
        let late = JsonState::new(&dir);
        SessionRegistry::save(&late, &Session::new(SessionId::new("late"), None)).unwrap();
        let again = SqliteState::new(&dir);
        assert_eq!(again.load(&SessionId::new("late")).unwrap(), None);
        assert!(late.has_files());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_fresh_directory_has_nothing_to_migrate() {
        let dir = scratch("fresh");
        let state = SqliteState::new(&dir);
        assert_eq!(state.summary().unwrap().migrated, None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_oldest_cases_are_forgotten_past_the_cap() {
        let dir = scratch("cap");
        let state = SqliteState::new(&dir);
        for i in 0..=KEPT_CASES {
            CaseStore::save(&state, &case(&format!("c{i}"), &format!("s{i}"), i as u64)).unwrap();
        }
        assert_eq!(state.summary().unwrap().cases as i64, KEPT_CASES);
        assert_eq!(state.last(Some(&SessionId::new("s0"))).unwrap(), None);
        assert!(state.last(Some(&SessionId::new("s1"))).unwrap().is_some());
        std::fs::remove_dir_all(dir).unwrap();
    }
}

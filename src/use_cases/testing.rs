//! In-memory fakes for every port, so use cases are tested in microseconds.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use crate::entities::*;
use crate::use_cases::ports::*;

pub struct FakeClock(pub Cell<Timestamp>);

impl FakeClock {
    pub fn at(ms: u64) -> Self {
        Self(Cell::new(Timestamp::from_millis(ms)))
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Timestamp {
        self.0.get()
    }
}

#[derive(Default)]
pub struct SequentialIds(Cell<u32>);

impl IdGenerator for SequentialIds {
    fn case_id(&self) -> CaseId {
        let n = self.0.get() + 1;
        self.0.set(n);
        CaseId::new(format!("case-{n}"))
    }
}

#[derive(Default)]
pub struct MemoryRegistry(pub RefCell<HashMap<String, Session>>);

impl SessionRegistry for MemoryRegistry {
    fn load(&self, id: &SessionId) -> Result<Option<Session>, RegistryError> {
        Ok(self.0.borrow().get(id.as_str()).cloned())
    }

    fn save(&self, session: &Session) -> Result<(), RegistryError> {
        self.0
            .borrow_mut()
            .insert(session.id().as_str().to_string(), session.clone());
        Ok(())
    }
}

#[derive(Default)]
pub struct MemoryCases(pub RefCell<Vec<FailureCase>>);

impl CaseStore for MemoryCases {
    fn save(&self, case: &FailureCase) -> Result<(), CaseStoreError> {
        self.0.borrow_mut().push(case.clone());
        Ok(())
    }

    fn last(&self, session: Option<&SessionId>) -> Result<Option<FailureCase>, CaseStoreError> {
        let cases = self.0.borrow();
        Ok(match session {
            Some(id) => cases
                .iter()
                .rev()
                .find(|c| c.session() == Some(id))
                .cloned(),
            None => cases.last().cloned(),
        })
    }
}

#[derive(Default)]
pub struct MemoryIgnores(pub RefCell<Vec<IgnoreEntry>>);

impl IgnoreStore for MemoryIgnores {
    fn entries(&self) -> Result<Vec<IgnoreEntry>, IgnoreStoreError> {
        Ok(self.0.borrow().clone())
    }

    fn replace(&self, entries: &[IgnoreEntry]) -> Result<(), IgnoreStoreError> {
        *self.0.borrow_mut() = entries.to_vec();
        Ok(())
    }
}

#[derive(Default)]
pub struct MemoryLearned(pub RefCell<LearnedBook>);

impl LearnedFixes for MemoryLearned {
    fn recall(&self, shape: &FailureShape) -> Result<Option<LearnedFix>, LearnedFixesError> {
        Ok(self.0.borrow().recall(shape).cloned())
    }

    fn accept(
        &self,
        shape: &FailureShape,
        command: &CommandLine,
        at: Timestamp,
    ) -> Result<LearnedFix, LearnedFixesError> {
        Ok(self.0.borrow_mut().accept(shape, command, at))
    }

    fn entries(&self) -> Result<Vec<LearnedFix>, LearnedFixesError> {
        Ok(self.0.borrow().entries())
    }

    fn forget(&self, program: Option<&str>) -> Result<usize, LearnedFixesError> {
        Ok(self.0.borrow_mut().forget(program))
    }
}

/// What every `LearnedFixes` store must do, the fake and the gateways alike.
pub fn learned_fixes_contract(store: &dyn LearnedFixes) {
    let shape = FailureShape::new("make test", ExitStatus::new(2));
    let fix = CommandLine::new("make -j4 test").unwrap();
    assert_eq!(store.recall(&shape).unwrap(), None, "nothing learned yet");
    let once = store
        .accept(&shape, &fix, Timestamp::from_millis(1))
        .unwrap();
    assert_eq!(once.acceptances(), 1);
    assert!(once.as_rule().is_none(), "once is not a rule");
    let twice = store
        .accept(&shape, &fix, Timestamp::from_millis(2))
        .unwrap();
    assert_eq!(twice.acceptances(), 2);
    assert_eq!(
        store
            .recall(&shape)
            .unwrap()
            .unwrap()
            .as_rule()
            .unwrap()
            .command(),
        &fix
    );
    let other = FailureShape::new("cargo publish", ExitStatus::new(101));
    store
        .accept(
            &other,
            &CommandLine::new("cargo publish --allow-dirty").unwrap(),
            Timestamp::from_millis(9),
        )
        .unwrap();
    let lines: Vec<String> = store
        .entries()
        .unwrap()
        .iter()
        .map(|e| e.shape().line().to_string())
        .collect();
    assert_eq!(
        lines,
        ["cargo publish", "make test"],
        "most recently taken first"
    );
    let restarted = store
        .accept(
            &shape,
            &CommandLine::new("make -j8 test").unwrap(),
            Timestamp::from_millis(10),
        )
        .unwrap();
    assert_eq!(
        restarted.acceptances(),
        1,
        "another fix for the same failure starts over"
    );
    assert_eq!(store.forget(Some("make")).unwrap(), 1);
    assert_eq!(store.recall(&shape).unwrap(), None);
    assert_eq!(store.entries().unwrap().len(), 1);
    assert_eq!(store.forget(None).unwrap(), 1);
    assert!(store.entries().unwrap().is_empty());
    for i in 0..=LearnedBook::CAPACITY {
        let shape = FailureShape::new(&format!("cmd{i}"), ExitStatus::new(1));
        let fix = CommandLine::new(format!("cmd{i} --fixed")).unwrap();
        store
            .accept(&shape, &fix, Timestamp::from_millis(i as u64 + 1))
            .unwrap();
    }
    assert_eq!(
        store.entries().unwrap().len(),
        LearnedBook::CAPACITY,
        "bounded"
    );
    assert_eq!(
        store
            .recall(&FailureShape::new("cmd0", ExitStatus::new(1)))
            .unwrap(),
        None,
        "the one not taken for longest went"
    );
    store.forget(None).unwrap();
}

#[derive(Default)]
pub struct FakeEnvironment {
    pub os: Option<Os>,
    pub executables: Vec<String>,
    pub entries: HashMap<String, Vec<DirEntry>>,
    pub aliases: Vec<AliasFact>,
    pub path_reads: Cell<u32>,
    pub docker_desktop: bool,
    pub docker_reads: Cell<u32>,
}

impl FakeEnvironment {
    pub fn with_executables(execs: &[&str]) -> Self {
        Self {
            os: Some(Os::Linux),
            executables: execs.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }
}

impl Environment for FakeEnvironment {
    fn os(&self) -> Option<Os> {
        self.os
    }

    fn executables(&self) -> Vec<String> {
        self.path_reads.set(self.path_reads.get() + 1);
        self.executables.clone()
    }

    fn entries(&self, dir: &str) -> Vec<DirEntry> {
        self.entries.get(dir).cloned().unwrap_or_default()
    }

    fn docker_desktop(&self) -> bool {
        self.docker_reads.set(self.docker_reads.get() + 1);
        self.docker_desktop
    }

    fn aliases(&self) -> Vec<AliasFact> {
        self.aliases.clone()
    }
}

pub struct MapSecrets(pub HashMap<String, String>);

impl MapSecrets {
    pub fn with(pairs: &[(&str, &str)]) -> Self {
        Self(
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        )
    }
}

impl Secrets for MapSecrets {
    fn lookup(&self, source: &KeySource) -> Option<String> {
        match source {
            KeySource::None => None,
            KeySource::Literal(k) => Some(k.clone()),
            KeySource::Env(name) | KeySource::Command(name) | KeySource::Keychain(name) => {
                self.0.get(name).cloned()
            }
        }
    }
}

/// Answers by model name; records every call with the key it was given.
#[derive(Default)]
pub struct ScriptedModels {
    pub answers: HashMap<String, Result<String, ModelError>>,
    pub calls: RefCell<Vec<(String, Option<String>, Prompt)>>,
    /// Models whose server is not running.
    pub down: Vec<String>,
    /// The tokens each model reports for an answer.
    pub usage: HashMap<String, Tokens>,
}

impl ScriptedModels {
    pub fn answering(pairs: &[(&str, Result<&str, ModelError>)]) -> Self {
        Self {
            answers: pairs
                .iter()
                .map(|(n, r)| (n.to_string(), r.clone().map(String::from)))
                .collect(),
            calls: RefCell::default(),
            down: Vec::new(),
            usage: HashMap::new(),
        }
    }

    /// The same, with the tokens each model reports.
    pub fn counting(mut self, pairs: &[(&str, Tokens)]) -> Self {
        self.usage = pairs.iter().map(|(n, t)| (n.to_string(), *t)).collect();
        self
    }

    pub fn asked(&self) -> Vec<String> {
        self.calls
            .borrow()
            .iter()
            .map(|(n, _, _)| n.clone())
            .collect()
    }
}

impl ModelGateway for ScriptedModels {
    fn is_reachable(&self, spec: &ModelSpec) -> bool {
        !self.down.contains(&spec.name)
    }

    fn complete(
        &self,
        spec: &ModelSpec,
        key: Option<&str>,
        prompt: &Prompt,
    ) -> Result<String, ModelError> {
        self.calls
            .borrow_mut()
            .push((spec.name.clone(), key.map(String::from), prompt.clone()));
        self.answers
            .get(&spec.name)
            .cloned()
            .unwrap_or_else(|| Err(ModelError::Unreachable("not scripted".into())))
    }

    /// The scripted answer, one word at a time, spaces attached.
    fn stream(
        &self,
        spec: &ModelSpec,
        key: Option<&str>,
        prompt: &Prompt,
        on_chunk: &mut dyn FnMut(&str),
    ) -> Result<String, ModelError> {
        let answer = self.complete(spec, key, prompt)?;
        let mut rest = answer.as_str();
        while !rest.is_empty() {
            let end = rest
                .find(' ')
                .map_or(rest.len(), |i| (i + 1).min(rest.len()));
            on_chunk(&rest[..end]);
            rest = &rest[end..];
        }
        Ok(answer)
    }

    fn answer(
        &self,
        spec: &ModelSpec,
        key: Option<&str>,
        prompt: &Prompt,
    ) -> Result<Answer, ModelError> {
        let text = self.complete(spec, key, prompt)?;
        Ok(Answer {
            text,
            tokens: self.usage.get(&spec.name).copied().unwrap_or_default(),
        })
    }
}

/// Every call kept in memory; a day's budget note too.
#[derive(Default)]
pub struct MemoryLedger {
    pub entries: RefCell<Vec<LedgerEntry>>,
    pub noted: RefCell<Vec<Day>>,
}

impl CostLedger for MemoryLedger {
    fn record(&self, entry: &LedgerEntry) -> Result<(), LedgerError> {
        self.entries.borrow_mut().push(entry.clone());
        Ok(())
    }

    fn since(&self, from: Timestamp) -> Result<Vec<LedgerEntry>, LedgerError> {
        Ok(self
            .entries
            .borrow()
            .iter()
            .filter(|e| e.at >= from)
            .cloned()
            .collect())
    }

    fn budget_noted(&self, day: Day) -> Result<bool, LedgerError> {
        Ok(self.noted.borrow().contains(&day))
    }

    fn note_budget(&self, day: Day) -> Result<(), LedgerError> {
        self.noted.borrow_mut().push(day);
        Ok(())
    }
}

/// Every event kept in memory.
#[derive(Default)]
pub struct MemoryScoreboard {
    pub events: RefCell<Vec<FixEvent>>,
}

impl Scoreboard for MemoryScoreboard {
    fn mark(&self, event: &FixEvent) -> Result<(), ScoreboardError> {
        self.events.borrow_mut().push(event.clone());
        Ok(())
    }

    fn since(&self, from: Timestamp) -> Result<Vec<FixEvent>, ScoreboardError> {
        Ok(self
            .events
            .borrow()
            .iter()
            .filter(|e| e.at >= from)
            .cloned()
            .collect())
    }
}

/// What every `Scoreboard` must do, run against an empty one.
pub fn scoreboard_contract(board: &dyn Scoreboard) {
    let event = |at: u64, kind: FixEventKind| FixEvent {
        at: Timestamp::from_millis(at),
        kind,
    };
    assert!(board.since(Timestamp::from_millis(0)).unwrap().is_empty());
    board.mark(&event(1_000, FixEventKind::Failure)).unwrap();
    board
        .mark(&event(
            1_000,
            FixEventKind::Offered(FixSource::Rule("command typo".into())),
        ))
        .unwrap();
    board
        .mark(&event(
            2_000,
            FixEventKind::Taken(FixSource::Model("local".into())),
        ))
        .unwrap();
    let all = board.since(Timestamp::from_millis(0)).unwrap();
    assert_eq!(
        all,
        vec![
            event(1_000, FixEventKind::Failure),
            event(
                1_000,
                FixEventKind::Offered(FixSource::Rule("command typo".into()))
            ),
            event(2_000, FixEventKind::Taken(FixSource::Model("local".into()))),
        ],
        "oldest first, every field kept"
    );
    assert_eq!(
        board.since(Timestamp::from_millis(2_000)).unwrap().len(),
        1,
        "from is inclusive"
    );
}

/// What every `CostLedger` must do, run against an empty one.
pub fn cost_ledger_contract(ledger: &dyn CostLedger) {
    let entry = |at: u64, model: &str| LedgerEntry {
        at: Timestamp::from_millis(at),
        model: model.into(),
        task: Task::QuickFix,
        tokens: Tokens::new(120, 30),
        cost: (model != "mystery").then_some(Money::from_micro_usd(at)),
        latency: Duration::from_millis(250),
    };
    assert!(ledger.since(Timestamp::from_millis(0)).unwrap().is_empty());
    ledger.record(&entry(1_000, "haiku")).unwrap();
    ledger.record(&entry(2_000, "mystery")).unwrap();
    ledger.record(&entry(3_000, "haiku")).unwrap();
    let all = ledger.since(Timestamp::from_millis(0)).unwrap();
    assert_eq!(
        all,
        vec![
            entry(1_000, "haiku"),
            entry(2_000, "mystery"),
            entry(3_000, "haiku")
        ],
        "oldest first, every field kept"
    );
    assert_eq!(all[1].cost, None, "an unpriced call stays unpriced");
    assert_eq!(
        ledger.since(Timestamp::from_millis(2_000)).unwrap().len(),
        2,
        "from is inclusive"
    );
    let today = Day::from_index(20_724);
    assert!(!ledger.budget_noted(today).unwrap());
    ledger.note_budget(today).unwrap();
    assert!(ledger.budget_noted(today).unwrap());
    assert!(
        !ledger.budget_noted(today.minus(1)).unwrap(),
        "one day at a time"
    );
}

#[derive(Default)]
pub struct RecordingLauncher {
    pub launched: RefCell<Vec<(String, String)>>,
    pub failure: Option<AgentError>,
}

impl AgentLauncher for RecordingLauncher {
    fn launch(&self, spec: &ModelSpec, brief: &str) -> Result<(), AgentError> {
        self.launched
            .borrow_mut()
            .push((spec.name.clone(), brief.to_string()));
        match &self.failure {
            Some(e) => Err(e.clone()),
            None => Ok(()),
        }
    }
}

/// A screen dump for any known pane; records what was asked.
#[derive(Default)]
pub struct FakeOutput {
    pub screen: Option<String>,
    pub asked: RefCell<Vec<(TerminalIdentity, usize)>>,
}

impl FakeOutput {
    pub fn showing(screen: &str) -> Self {
        Self {
            screen: Some(screen.to_string()),
            asked: RefCell::default(),
        }
    }
}

impl OutputSource for FakeOutput {
    fn recent(&self, terminal: &TerminalIdentity, lines: usize) -> Option<String> {
        self.asked.borrow_mut().push((terminal.clone(), lines));
        self.screen.clone()
    }
}

#[derive(Default)]
pub struct MemoryNotifier {
    pub delivered: RefCell<Vec<(SessionId, Message)>>,
}

impl Notifier for MemoryNotifier {
    fn deliver(&self, session: &SessionId, message: Message) -> Result<(), NotifyError> {
        self.delivered.borrow_mut().push((session.clone(), message));
        Ok(())
    }
}

pub fn spec(name: &str, provider: Provider, tier: Tier) -> ModelSpec {
    ModelSpec {
        name: name.into(),
        provider,
        model: name.into(),
        base_url: None,
        key: KeySource::None,
        tier,
        timeout: None,
        max_output_tokens: None,
    }
}

pub fn outcome(text: &str, code: i32) -> CommandOutcome {
    CommandOutcome::new(CommandLine::new(text).unwrap(), ExitStatus::new(code))
}

pub fn case(text: &str, code: i32, session: Option<&str>) -> FailureCase {
    FailureCase::new(
        CaseId::new("c"),
        Timestamp::from_millis(0),
        outcome(text, code),
        Some("/w".into()),
    )
    .with_session(session.map(SessionId::new))
}

/// A keychain in a map: stores under the account, reads back through
/// `Secrets` for `{ keychain = true }`, fails on demand.
#[derive(Default)]
pub struct MemorySecretStore {
    pub entries: RefCell<HashMap<String, String>>,
    pub failure: Option<SecretStoreError>,
}

impl SecretStore for MemorySecretStore {
    fn store(&self, account: &str, key: &SecretKey) -> Result<(), SecretStoreError> {
        if let Some(failure) = &self.failure {
            return Err(failure.clone());
        }
        self.entries
            .borrow_mut()
            .insert(account.to_string(), key.expose().to_string());
        Ok(())
    }
}

impl Secrets for MemorySecretStore {
    fn lookup(&self, source: &KeySource) -> Option<String> {
        match source {
            KeySource::Keychain(account) => self.entries.borrow().get(account).cloned(),
            _ => None,
        }
    }
}

/// What every `SecretStore` that can also be read must do: a stored key
/// is found under its account, a second store replaces it, accounts are
/// independent.
pub fn secret_store_contract<S: SecretStore + Secrets>(store: &S) {
    let first = SecretKey::new("sk-first").unwrap();
    let second = SecretKey::new("sk-second").unwrap();
    assert_eq!(store.lookup(&KeySource::Keychain("a".into())), None);
    store.store("a", &first).unwrap();
    assert_eq!(
        store.lookup(&KeySource::Keychain("a".into())).as_deref(),
        Some("sk-first")
    );
    store.store("a", &second).unwrap();
    assert_eq!(
        store.lookup(&KeySource::Keychain("a".into())).as_deref(),
        Some("sk-second"),
        "the second store replaces the first"
    );
    assert_eq!(store.lookup(&KeySource::Keychain("b".into())), None);
    store.store("b", &first).unwrap();
    assert_eq!(
        store.lookup(&KeySource::Keychain("a".into())).as_deref(),
        Some("sk-second"),
        "accounts are independent"
    );
    assert_eq!(
        store.lookup(&KeySource::Env("a".into())),
        None,
        "the keychain answers for keychain sources only"
    );
}

#[cfg(test)]
mod secret_store_tests {
    use super::*;

    #[test]
    fn the_in_memory_keychain_obeys_the_contract() {
        secret_store_contract(&MemorySecretStore::default());
    }
}

/// What every storage gateway must honour, and the fakes with them: the
/// same tests run against the in-memory store, the JSON files and SQLite.
pub mod storage_contract {
    use super::*;

    fn failure(text: &str, code: i32) -> CommandOutcome {
        CommandOutcome::new(CommandLine::new(text).unwrap(), ExitStatus::new(code))
    }

    pub fn sessions_round_trip_with_their_shell_and_history(store: &impl SessionRegistry) {
        let id = SessionId::new("42");
        assert_eq!(store.load(&id).unwrap(), None);
        let mut session = Session::new(id.clone(), Some(Shell::Fish));
        session.remember(failure("ls", 0));
        session.remember(failure("make", 2).lasting(Duration::from_secs(3)));
        store.save(&session).unwrap();
        assert_eq!(store.load(&id).unwrap(), Some(session.clone()));
        session.remember(failure("make -j4", 0));
        store.save(&session).unwrap();
        assert_eq!(
            store.load(&id).unwrap(),
            Some(session),
            "saving again replaces what was there"
        );
    }

    pub fn a_pipelines_statuses_round_trip_with_the_outcome(store: &impl SessionRegistry) {
        let pipeline = failure("gti status | head", 0)
            .in_pipeline(vec![ExitStatus::new(127), ExitStatus::new(0)]);
        let session = Session::with_recent(SessionId::new("p"), None, vec![pipeline.clone()]);
        store.save(&session).unwrap();
        let back = store.load(&SessionId::new("p")).unwrap().unwrap();
        assert_eq!(back.recent(), &[pipeline]);
    }

    pub fn the_last_case_is_kept_per_session_and_overall(store: &impl CaseStore) {
        let a = FailureCase::new(
            CaseId::new("a"),
            Timestamp::from_millis(1),
            failure("make", 2),
            Some("/w".into()),
        )
        .with_session(Some(SessionId::new("s1")))
        .with_recent(vec![CommandLine::new("ls").unwrap()])
        .with_output("boom".into())
        .with_proposal(Some(Fix::new(
            CommandLine::new("make -j4").unwrap(),
            Confidence::new(0.6),
            FixSource::Model("local".into()),
            "suggested by local",
        )))
        .with_explanation(Explanation::new("local", "the target is missing"));
        let b = FailureCase::new(
            CaseId::new("b"),
            Timestamp::from_millis(2),
            failure("cargo", 101),
            None,
        )
        .with_session(Some(SessionId::new("s2/odd id")));
        store.save(&a).unwrap();
        store.save(&b).unwrap();
        assert_eq!(
            store.last(Some(&SessionId::new("s1"))).unwrap(),
            Some(a.clone())
        );
        assert_eq!(
            store.last(Some(&SessionId::new("s2/odd id"))).unwrap(),
            Some(b.clone())
        );
        assert_eq!(store.last(None).unwrap(), Some(b.clone()));
        assert_eq!(store.last(Some(&SessionId::new("s3"))).unwrap(), None);
        assert!(store.still_current(&a).unwrap());
        let a_again = a
            .clone()
            .with_explanation(Explanation::new("cloud", "later"));
        store.save(&a_again).unwrap();
        assert_eq!(
            store.last(Some(&SessionId::new("s1"))).unwrap(),
            Some(a_again.clone()),
            "saving a case again replaces it"
        );
        assert_eq!(
            store.last(None).unwrap(),
            Some(a_again),
            "and makes it the last overall: callers check still_current first"
        );
        assert!(
            store.still_current(&b).unwrap(),
            "b is still the last of its own session"
        );
    }

    pub fn the_ignore_list_round_trips(store: &impl IgnoreStore) {
        assert!(store.entries().unwrap().is_empty());
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
        store.replace(&entries).unwrap();
        assert_eq!(store.entries().unwrap(), entries);
        store.replace(&entries[1..2]).unwrap();
        assert_eq!(
            store.entries().unwrap(),
            entries[1..2].to_vec(),
            "replaced, not appended"
        );
        store.replace(&[]).unwrap();
        assert!(store.entries().unwrap().is_empty());
    }

    /// Every test of the contract, for a store that implements the three ports.
    pub fn everything(store: &(impl SessionRegistry + CaseStore + IgnoreStore)) {
        sessions_round_trip_with_their_shell_and_history(store);
        a_pipelines_statuses_round_trip_with_the_outcome(store);
        the_last_case_is_kept_per_session_and_overall(store);
        the_ignore_list_round_trips(store);
    }
}

#[cfg(test)]
mod contract_tests {
    use super::*;

    #[test]
    fn the_in_memory_fakes_honour_the_storage_contract() {
        storage_contract::sessions_round_trip_with_their_shell_and_history(
            &MemoryRegistry::default(),
        );
        storage_contract::a_pipelines_statuses_round_trip_with_the_outcome(
            &MemoryRegistry::default(),
        );
        storage_contract::the_last_case_is_kept_per_session_and_overall(&MemoryCases::default());
        storage_contract::the_ignore_list_round_trips(&MemoryIgnores::default());
    }
}

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
pub struct FakeEnvironment {
    pub os: Option<Os>,
    pub executables: Vec<String>,
    pub entries: HashMap<String, Vec<DirEntry>>,
    pub path_reads: Cell<u32>,
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

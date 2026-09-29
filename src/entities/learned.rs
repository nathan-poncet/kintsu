//! What repeated acceptances teach: a fix the user took twice for the
//! same failure becomes a rule of their own, instant and offline like the
//! built-in ones.

use std::fmt;

use crate::entities::{
    CommandLine, CommandOutcome, Confidence, Danger, ExitStatus, FailureCase, Fix, FixSource,
    Timestamp,
};

/// The name learned fixes carry as their rule.
pub const LEARNED_RULE: &str = "learned";

/// The failure a learned fix answers: the command line's words and its
/// exit status, the same pair that makes two failures one duplicate.
/// `make test` and `make build` are two shapes: a fix for one says
/// nothing about the other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailureShape {
    line: String,
    status: ExitStatus,
}

impl FailureShape {
    /// The shape of a finished command line.
    pub fn of(outcome: &CommandOutcome) -> Self {
        Self::new(outcome.command().as_str(), outcome.status())
    }

    /// From stored parts; the words are normalised again, so an edited
    /// file still matches.
    pub fn new(line: &str, status: ExitStatus) -> Self {
        Self {
            line: line.split_whitespace().collect::<Vec<_>>().join(" "),
            status,
        }
    }

    /// The words, one space apart.
    pub fn line(&self) -> &str {
        &self.line
    }

    /// The first word, for forgetting by program.
    pub fn program(&self) -> &str {
        self.line.split_whitespace().next().unwrap_or_default()
    }

    /// The exit status.
    pub fn status(&self) -> ExitStatus {
        self.status
    }
}

impl fmt::Display for FailureShape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.line, self.status.code())
    }
}

/// A fix the user took for a shape of failure, and how often.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LearnedFix {
    shape: FailureShape,
    command: CommandLine,
    acceptances: u32,
    last_accepted: Timestamp,
}

impl LearnedFix {
    /// Taken this many times before it is offered as a rule.
    pub const MINIMUM_ACCEPTANCES: u32 = 2;

    /// The first time a fix is taken.
    pub fn first(shape: FailureShape, command: CommandLine, at: Timestamp) -> Self {
        Self {
            shape,
            command,
            acceptances: 1,
            last_accepted: at,
        }
    }

    /// Rebuilt from storage.
    pub fn restore(
        shape: FailureShape,
        command: CommandLine,
        acceptances: u32,
        last_accepted: Timestamp,
    ) -> Self {
        Self {
            shape,
            command,
            acceptances: acceptances.max(1),
            last_accepted,
        }
    }

    /// Taken once more.
    pub fn accepted(mut self, at: Timestamp) -> Self {
        self.acceptances = self.acceptances.saturating_add(1);
        self.last_accepted = at;
        self
    }

    pub fn shape(&self) -> &FailureShape {
        &self.shape
    }

    pub fn command(&self) -> &CommandLine {
        &self.command
    }

    pub fn acceptances(&self) -> u32 {
        self.acceptances
    }

    pub fn last_accepted(&self) -> Timestamp {
        self.last_accepted
    }

    /// The fix as a rule, once it was taken twice: sure enough to be
    /// pre-typed at 0.8, surer from three times.
    pub fn as_rule(&self) -> Option<Fix> {
        if self.acceptances < Self::MINIMUM_ACCEPTANCES {
            return None;
        }
        let confidence = if self.acceptances >= 3 { 0.9 } else { 0.8 };
        Some(Fix::new(
            self.command.clone(),
            Confidence::new(confidence),
            FixSource::Rule(LEARNED_RULE.into()),
            format!(
                "You took this fix {} times for `{}`.",
                self.acceptances,
                self.shape.line()
            ),
        ))
    }
}

/// Every learned fix, and the rules for changing the list: one entry per
/// shape, a different fix for a shape starts over, the list is bounded and
/// the entry not taken for longest goes first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LearnedBook(Vec<LearnedFix>);

impl LearnedBook {
    /// How many entries are kept.
    pub const CAPACITY: usize = 500;

    pub fn from_entries(entries: Vec<LearnedFix>) -> Self {
        Self(entries)
    }

    /// The entries, most recently taken first.
    pub fn entries(&self) -> Vec<LearnedFix> {
        let mut entries = self.0.clone();
        entries.sort_by(|a, b| b.last_accepted.cmp(&a.last_accepted));
        entries
    }

    pub fn recall(&self, shape: &FailureShape) -> Option<&LearnedFix> {
        self.0.iter().find(|e| &e.shape == shape)
    }

    /// One more acceptance; the entry as it now stands.
    pub fn accept(
        &mut self,
        shape: &FailureShape,
        command: &CommandLine,
        at: Timestamp,
    ) -> LearnedFix {
        let entry = match self.0.iter().position(|e| &e.shape == shape) {
            Some(i) if self.0[i].command.words() == command.words() => {
                self.0.remove(i).accepted(at)
            }
            Some(i) => {
                self.0.remove(i);
                LearnedFix::first(shape.clone(), command.clone(), at)
            }
            None => LearnedFix::first(shape.clone(), command.clone(), at),
        };
        self.0.push(entry.clone());
        while self.0.len() > Self::CAPACITY {
            let oldest = self
                .0
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| e.last_accepted)
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.0.remove(oldest);
        }
        entry
    }

    /// Forgets one program's entries, or all of them; how many went.
    pub fn forget(&mut self, program: Option<&str>) -> usize {
        let before = self.0.len();
        match program {
            Some(p) => self.0.retain(|e| e.shape.program() != p),
            None => self.0.clear(),
        }
        before - self.0.len()
    }
}

/// The proposal of `case` when `next`, the command line that followed it,
/// is that proposal and succeeded: the user took it. Only fixes worth
/// learning count: harmless ones the built-in rules do not already
/// pre-type, and learned ones getting surer.
pub fn accepted_proposal<'a>(case: &'a FailureCase, next: &CommandOutcome) -> Option<&'a Fix> {
    let fix = case.proposal()?;
    let taken = next.status().is_success() && next.command().words() == fix.command().words();
    (taken && teaches(fix)).then_some(fix)
}

fn teaches(fix: &Fix) -> bool {
    if *fix.danger() != Danger::None {
        return false;
    }
    match fix.source() {
        FixSource::Model(_) => true,
        FixSource::Rule(name) => name == LEARNED_RULE || !fix.is_ghostable(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::CaseId;

    fn outcome(text: &str, code: i32) -> CommandOutcome {
        CommandOutcome::new(CommandLine::new(text).unwrap(), ExitStatus::new(code))
    }
    fn line(text: &str) -> CommandLine {
        CommandLine::new(text).unwrap()
    }
    fn at(ms: u64) -> Timestamp {
        Timestamp::from_millis(ms)
    }
    fn case_with(text: &str, code: i32, proposal: Fix) -> FailureCase {
        FailureCase::new(CaseId::new("c"), at(0), outcome(text, code), None)
            .with_proposal(Some(proposal))
    }
    fn model_fix(text: &str) -> Fix {
        Fix::new(
            line(text),
            Confidence::new(0.6),
            FixSource::Model("local".into()),
            "",
        )
    }

    #[test]
    fn a_shape_is_the_words_and_the_status_whatever_the_spacing() {
        let shape = FailureShape::of(&outcome("make   test ", 2));
        assert_eq!(shape, FailureShape::new("make test", ExitStatus::new(2)));
        assert_eq!(shape.program(), "make");
        assert_eq!(shape.to_string(), "make test (2)");
        assert_ne!(shape, FailureShape::of(&outcome("make build", 2)));
        assert_ne!(shape, FailureShape::of(&outcome("make test", 1)));
    }

    #[test]
    fn a_fix_becomes_a_rule_once_taken_twice_and_surer_from_three() {
        let shape = FailureShape::of(&outcome("make test", 2));
        let once = LearnedFix::first(shape, line("make -j4 test"), at(1));
        assert!(once.as_rule().is_none());
        let twice = once.accepted(at(2));
        let rule = twice.as_rule().unwrap();
        assert_eq!(rule.command().as_str(), "make -j4 test");
        assert_eq!(rule.source(), &FixSource::Rule("learned".into()));
        assert!(rule.is_ghostable(), "0.8 and harmless");
        assert!(rule.rationale().contains("2 times"));
        let thrice = twice.accepted(at(3));
        assert_eq!(thrice.as_rule().unwrap().confidence().value(), 0.9);
        assert_eq!(thrice.last_accepted(), at(3));
    }

    #[test]
    fn taking_the_proposal_and_succeeding_is_accepting_it() {
        let case = case_with("make test", 2, model_fix("make -j4 test"));
        assert!(accepted_proposal(&case, &outcome("make  -j4 test", 0)).is_some());
        assert!(
            accepted_proposal(&case, &outcome("make -j4 test", 2)).is_none(),
            "it failed too"
        );
        assert!(
            accepted_proposal(&case, &outcome("make -j8 test", 0)).is_none(),
            "another command"
        );
        let bare = FailureCase::new(CaseId::new("c"), at(0), outcome("make test", 2), None);
        assert!(
            accepted_proposal(&bare, &outcome("make", 0)).is_none(),
            "no proposal"
        );
    }

    #[test]
    fn pre_typed_rule_fixes_and_dangerous_ones_teach_nothing() {
        let typo = Fix::new(
            line("git status"),
            Confidence::new(0.9),
            FixSource::Rule("command typo".into()),
            "",
        );
        assert!(
            accepted_proposal(
                &case_with("gti status", 127, typo),
                &outcome("git status", 0)
            )
            .is_none(),
            "already instant"
        );
        let unsure = Fix::new(
            line("git commit --amend"),
            Confidence::new(0.75),
            FixSource::Rule("one dash".into()),
            "",
        );
        assert!(
            accepted_proposal(
                &case_with("git commit -amend", 1, unsure),
                &outcome("git commit --amend", 0)
            )
            .is_some(),
            "a rule's guess about intent is worth confirming"
        );
        let learned = LearnedFix::first(
            FailureShape::of(&outcome("make test", 2)),
            line("make -j4 test"),
            at(1),
        )
        .accepted(at(2))
        .as_rule()
        .unwrap();
        assert!(
            accepted_proposal(
                &case_with("make test", 2, learned),
                &outcome("make -j4 test", 0)
            )
            .is_some(),
            "a learned fix keeps counting"
        );
        let destructive = model_fix("rm -rf build && make");
        assert!(
            accepted_proposal(
                &case_with("make", 2, destructive),
                &outcome("rm -rf build && make", 0)
            )
            .is_none()
        );
    }

    #[test]
    fn the_book_keeps_one_fix_per_shape_and_starts_over_on_another_fix() {
        let mut book = LearnedBook::default();
        let shape = FailureShape::of(&outcome("make test", 2));
        assert_eq!(
            book.accept(&shape, &line("make -j4 test"), at(1))
                .acceptances(),
            1
        );
        assert_eq!(
            book.accept(&shape, &line("make -j4 test"), at(2))
                .acceptances(),
            2
        );
        assert_eq!(book.recall(&shape).unwrap().acceptances(), 2);
        assert_eq!(
            book.accept(&shape, &line("make -j8 test"), at(3))
                .acceptances(),
            1,
            "another fix for the same failure starts over"
        );
        assert_eq!(book.entries().len(), 1);
        assert!(
            book.recall(&FailureShape::of(&outcome("make build", 2)))
                .is_none()
        );
    }

    #[test]
    fn the_book_lists_the_most_recent_first_forgets_by_program_and_stays_bounded() {
        let mut book = LearnedBook::default();
        book.accept(
            &FailureShape::of(&outcome("make test", 2)),
            &line("make -j4 test"),
            at(5),
        );
        book.accept(
            &FailureShape::of(&outcome("git push", 128)),
            &line("git push -u origin main"),
            at(9),
        );
        book.accept(
            &FailureShape::of(&outcome("make", 2)),
            &line("make all"),
            at(7),
        );
        let lines: Vec<String> = book
            .entries()
            .iter()
            .map(|e| e.shape().line().to_string())
            .collect();
        assert_eq!(lines, ["git push", "make", "make test"]);
        assert_eq!(book.forget(Some("make")), 2);
        assert_eq!(book.entries().len(), 1);
        assert_eq!(book.forget(None), 1);
        for i in 0..=LearnedBook::CAPACITY {
            let shape = FailureShape::new(&format!("cmd{i}"), ExitStatus::new(1));
            book.accept(&shape, &line(&format!("cmd{i} --fixed")), at(i as u64 + 1));
        }
        assert_eq!(book.entries().len(), LearnedBook::CAPACITY);
        assert!(
            book.recall(&FailureShape::new("cmd0", ExitStatus::new(1)))
                .is_none(),
            "the one not taken for longest went"
        );
    }
}

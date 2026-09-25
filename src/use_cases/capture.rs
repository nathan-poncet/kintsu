//! After the bubble: read the failed command's output from the terminal,
//! keep it with the case, so `why`, `fix`, `agent` and `privacy` have it.
//! Never on the quiet path: only an offered case is worth a capture.

use crate::entities::{FailureCase, Settings, TerminalIdentity, output_after};
use crate::use_cases::ports::{CaseStore, CaseStoreError, OutputSource};

pub struct CaptureOutput<'a> {
    pub settings: &'a Settings,
    pub output: &'a dyn OutputSource,
    pub cases: &'a dyn CaseStore,
}

/// A margin of lines read beyond the cap, so the command's echo is found.
const ECHO_MARGIN: usize = 50;

impl CaptureOutput<'_> {
    /// The case with its output when a source gave one; the same case
    /// otherwise. Saved when it changed.
    pub fn run(
        &self,
        case: FailureCase,
        terminal: &TerminalIdentity,
    ) -> Result<FailureCase, CaseStoreError> {
        let max_lines = self.settings.capture.max_lines;
        if max_lines == 0 || !terminal.is_known() {
            return Ok(case);
        }
        let Some(screen) = self.output.recent(terminal, max_lines + ECHO_MARGIN) else {
            return Ok(case);
        };
        let Some(output) = output_after(&screen, case.outcome().command().as_str(), max_lines)
        else {
            return Ok(case);
        };
        let case = case.with_output(output);
        if self.cases.still_current(&case)? {
            self.cases.save(&case)?;
        }
        Ok(case)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::SessionId;
    use crate::use_cases::testing::*;

    fn herdr() -> TerminalIdentity {
        TerminalIdentity {
            herdr_pane: Some("wS:p1".into()),
            ..Default::default()
        }
    }

    #[test]
    fn the_output_is_cut_out_of_the_screen_and_saved_with_the_case() {
        let cases = MemoryCases::default();
        let screen =
            FakeOutput::showing("$ make test\nmake: *** No rule to make target 'test'.  Stop.\n");
        let settings = Settings::default();
        let uc = CaptureOutput {
            settings: &settings,
            output: &screen,
            cases: &cases,
        };
        let case = uc.run(case("make test", 2, Some("42")), &herdr()).unwrap();
        assert_eq!(
            case.output(),
            Some("make: *** No rule to make target 'test'.  Stop.")
        );
        assert_eq!(
            cases
                .last(Some(&SessionId::new("42")))
                .unwrap()
                .unwrap()
                .output(),
            case.output()
        );
        assert_eq!(
            screen.asked.borrow().as_slice(),
            &[(herdr(), settings.capture.max_lines + 50)]
        );
    }

    #[test]
    fn a_slow_read_does_not_bring_back_a_failure_the_shell_moved_past() {
        use crate::entities::{CaseId, Timestamp};
        let cases = MemoryCases::default();
        let old = case("make test", 2, Some("42"));
        cases.save(&old).unwrap();
        let newer = FailureCase::new(
            CaseId::new("c2"),
            Timestamp::from_millis(1),
            outcome("ls nope", 1),
            None,
        )
        .with_session(Some(SessionId::new("42")));
        cases.save(&newer).unwrap();
        let screen = FakeOutput::showing("$ make test\nmake: boom\n");
        let settings = Settings::default();
        let uc = CaptureOutput {
            settings: &settings,
            output: &screen,
            cases: &cases,
        };
        let read = uc.run(old, &herdr()).unwrap();
        assert_eq!(
            read.output(),
            Some("make: boom"),
            "the caller still gets it"
        );
        let last = cases.last(Some(&SessionId::new("42"))).unwrap().unwrap();
        assert_eq!((last.id().as_str(), last.output()), ("c2", None));
    }

    #[test]
    fn nothing_is_read_for_an_unknown_pane_a_zero_cap_or_a_silent_source() {
        let cases = MemoryCases::default();
        let screen = FakeOutput::showing("$ make\nboom\n");
        let settings = Settings::default();
        let uc = CaptureOutput {
            settings: &settings,
            output: &screen,
            cases: &cases,
        };
        assert_eq!(
            uc.run(case("make", 2, Some("42")), &TerminalIdentity::default())
                .unwrap()
                .output(),
            None
        );
        let mut off = Settings::default();
        off.capture.max_lines = 0;
        let quiet = CaptureOutput {
            settings: &off,
            ..uc
        };
        assert_eq!(
            quiet
                .run(case("make", 2, Some("42")), &herdr())
                .unwrap()
                .output(),
            None
        );
        assert!(screen.asked.borrow().is_empty());
        let silent = FakeOutput::default();
        let none = CaptureOutput {
            output: &silent,
            ..uc
        };
        assert_eq!(
            none.run(case("make", 2, Some("42")), &herdr())
                .unwrap()
                .output(),
            None
        );
        assert!(
            cases.last(None).unwrap().is_none(),
            "nothing saved when nothing was read"
        );
    }
}

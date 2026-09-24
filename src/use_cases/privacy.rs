//! `kintsu privacy`: exactly what a model or an agent would receive.

use thiserror::Error;

use crate::entities::{CaseDocument, SessionId, case_document};
use crate::use_cases::ports::{CaseStore, CaseStoreError};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PrivacyError {
    #[error("no failure in this shell yet")]
    NoCase,
    #[error(transparent)]
    Cases(#[from] CaseStoreError),
}

/// Shows the redacted document of the last case.
pub struct Privacy<'a> {
    pub cases: &'a dyn CaseStore,
}

impl Privacy<'_> {
    pub fn run(&self, session: Option<&SessionId>) -> Result<CaseDocument, PrivacyError> {
        let case = self.cases.last(session)?.ok_or(PrivacyError::NoCase)?;
        Ok(case_document(&case))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::use_cases::testing::*;

    #[test]
    fn the_document_is_what_would_leave_the_machine() {
        let cases = MemoryCases::default();
        let uc = Privacy { cases: &cases };
        assert_eq!(uc.run(None).unwrap_err(), PrivacyError::NoCase);
        cases
            .save(&case("curl -u me:hunter2secret https://x", 22, None))
            .unwrap();
        let doc = uc.run(None).unwrap();
        assert!(
            doc.text
                .contains("```sh\ncurl -u me:hunter2secret https://x\n```"),
            "-u is not a URL password; left as is"
        );
        cases
            .save(&case("curl https://me:hunter2secret@x", 22, None))
            .unwrap();
        let doc = uc.run(None).unwrap();
        assert!(doc.text.contains("https://me:••••••••@x"));
        assert_eq!(doc.redactions, 1);
    }
}

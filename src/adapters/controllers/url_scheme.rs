//! `kintsu://act?case=<id>&do=<action>`: the URL behind every clickable
//! word of the bubble, as the operating system hands it to `kintsu open`.

use thiserror::Error;

use crate::entities::{Action, CaseId};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum UrlError {
    #[error("not a kintsu:// URL")]
    NotKintsu,
    #[error("unknown path `{0}` (only act)")]
    UnknownPath(String),
    #[error("the URL lacks `{0}`")]
    Missing(&'static str),
    #[error("unknown action `{0}`")]
    UnknownAction(String),
}

/// The case and the action a click asks for.
pub fn parse_act_url(url: &str) -> Result<(CaseId, Action), UrlError> {
    let rest = url
        .trim()
        .strip_prefix("kintsu://")
        .ok_or(UrlError::NotKintsu)?;
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    if path.trim_end_matches('/') != "act" {
        return Err(UrlError::UnknownPath(path.to_string()));
    }
    let mut case = None;
    let mut action = None;
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        match key {
            "case" => case = Some(value.to_string()),
            "do" => action = Some(value.to_string()),
            _ => {}
        }
    }
    let case = case
        .filter(|c| !c.is_empty())
        .ok_or(UrlError::Missing("case"))?;
    let action = action.ok_or(UrlError::Missing("do"))?;
    let action = Action::from_name(&action).ok_or(UrlError::UnknownAction(action))?;
    Ok((CaseId::new(case), action))
}

/// The URL a word of the bubble links to.
pub fn act_url(case: &CaseId, action: Action) -> String {
    format!("kintsu://act?case={}&do={}", case.as_str(), action.name())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_round_trips_through_the_parser() {
        let url = act_url(&CaseId::new("3e39"), Action::Why);
        assert_eq!(url, "kintsu://act?case=3e39&do=why");
        assert_eq!(
            parse_act_url(&url).unwrap(),
            (CaseId::new("3e39"), Action::Why)
        );
        assert_eq!(
            parse_act_url("kintsu://act/?do=ignore&case=c&x=1").unwrap(),
            (CaseId::new("c"), Action::Ignore)
        );
    }

    #[test]
    fn bad_urls_are_named() {
        assert_eq!(
            parse_act_url("https://example.com"),
            Err(UrlError::NotKintsu)
        );
        assert_eq!(
            parse_act_url("kintsu://run?case=c&do=why"),
            Err(UrlError::UnknownPath("run".into()))
        );
        assert_eq!(
            parse_act_url("kintsu://act?do=why"),
            Err(UrlError::Missing("case"))
        );
        assert_eq!(
            parse_act_url("kintsu://act?case=c"),
            Err(UrlError::Missing("do"))
        );
        assert_eq!(
            parse_act_url("kintsu://act?case=c&do=dance"),
            Err(UrlError::UnknownAction("dance".into()))
        );
    }
}

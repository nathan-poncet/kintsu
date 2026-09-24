//! What must never leave the machine, and how it is masked.

use std::sync::OnceLock;

use regex::Regex;

/// What kind of secret a pattern caught.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretKind {
    /// A bearer or basic authorization value.
    Authorization,
    /// A vendor API key with a recognisable prefix.
    ApiKey,
    /// A JSON Web Token.
    Jwt,
    /// A password inside a URL.
    UrlPassword,
    /// A `SOMETHING_TOKEN=value` style assignment.
    Assignment,
    /// A PEM private key block.
    PrivateKey,
}

/// One redaction that happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// The kind of secret.
    pub kind: SecretKind,
}

/// Text with its secrets masked, and the list of what was masked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redacted {
    text: String,
    findings: Vec<Finding>,
}

impl Redacted {
    /// The text, safe to send.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// What was masked, in order of appearance.
    pub fn findings(&self) -> &[Finding] {
        &self.findings
    }
}

const MASK: &str = "••••••••";

struct Pattern {
    kind: SecretKind,
    regex: &'static str,
    /// Which capture group holds the secret; 0 masks the whole match.
    group: usize,
}

const PATTERNS: &[Pattern] = &[
    Pattern {
        kind: SecretKind::PrivateKey,
        regex: r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----",
        group: 0,
    },
    Pattern {
        kind: SecretKind::Authorization,
        regex: r"(?i)(authorization:\s*(?:bearer|basic|token)\s+)([A-Za-z0-9._~+/=-]{8,})",
        group: 2,
    },
    Pattern {
        kind: SecretKind::Authorization,
        regex: r"(?i)(\bbearer\s+)([A-Za-z0-9._~+/=-]{16,})",
        group: 2,
    },
    Pattern {
        kind: SecretKind::Jwt,
        regex: r"\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\b",
        group: 0,
    },
    Pattern {
        kind: SecretKind::ApiKey,
        regex: r"\b(sk-(?:live|test|ant|proj)?-?[A-Za-z0-9_-]{12,}|ghp_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|gho_[A-Za-z0-9]{20,}|xox[abp]-[A-Za-z0-9-]{10,}|AKIA[0-9A-Z]{16}|AIza[0-9A-Za-z_-]{30,}|npm_[A-Za-z0-9]{30,}|glpat-[A-Za-z0-9_-]{20,})\b",
        group: 0,
    },
    Pattern {
        kind: SecretKind::UrlPassword,
        regex: r"(://[^/\s:@]+:)([^@\s/•]+)(@)",
        group: 2,
    },
    Pattern {
        kind: SecretKind::Assignment,
        regex: r"(?i)\b([A-Z0-9_]*(?:TOKEN|SECRET|PASSWORD|PASSWD|API_KEY|APIKEY|PRIVATE_KEY|ACCESS_KEY)[A-Z0-9_]*\s*[=:]\s*[\x22']?)([^\s\x22'•]{6,})",
        group: 2,
    },
];

fn compiled() -> &'static Vec<(SecretKind, Regex, usize)> {
    static COMPILED: OnceLock<Vec<(SecretKind, Regex, usize)>> = OnceLock::new();
    COMPILED.get_or_init(|| {
        PATTERNS
            .iter()
            .map(|p| (p.kind, Regex::new(p.regex).expect("pattern"), p.group))
            .collect()
    })
}

/// Masks every secret the patterns recognise; the rest of the text is kept
/// byte for byte.
pub fn redact(text: &str) -> Redacted {
    let mut out = text.to_string();
    let mut findings = Vec::new();
    for (kind, re, group) in compiled() {
        let mut next = String::with_capacity(out.len());
        let mut last = 0;
        for caps in re.captures_iter(&out) {
            let m = caps.get(*group).or_else(|| caps.get(0)).expect("group");
            next.push_str(&out[last..m.start()]);
            next.push_str(MASK);
            last = m.end();
            findings.push(Finding { kind: *kind });
        }
        next.push_str(&out[last..]);
        out = next;
    }
    Redacted {
        text: out,
        findings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<SecretKind> {
        redact(text).findings().iter().map(|f| f.kind).collect()
    }

    #[test]
    fn bearer_tokens_are_masked_and_the_rest_is_kept() {
        let r = redact(
            "curl -H 'Authorization: Bearer sk-live-8f3aabcdef123456c21e' https://api.acme.dev",
        );
        assert_eq!(
            r.text(),
            "curl -H 'Authorization: Bearer ••••••••' https://api.acme.dev"
        );
        assert_eq!(r.findings().len(), 1);
    }

    #[test]
    fn vendor_keys_jwts_and_url_passwords_are_recognised() {
        assert_eq!(
            kinds("export OPENAI=sk-proj-abcdefghijklmnopqrstuv"),
            vec![SecretKind::ApiKey]
        );
        assert_eq!(
            kinds("token ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123"),
            vec![SecretKind::ApiKey]
        );
        assert_eq!(
            kinds(
                "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U"
            ),
            vec![SecretKind::Jwt]
        );
        let r = redact("postgres://kintsu:hunter2secret@db.local:5432/app");
        assert_eq!(r.text(), "postgres://kintsu:••••••••@db.local:5432/app");
        assert_eq!(
            kinds("NPM_TOKEN=npm_abcdefghijklmnopqrstuvwxyz0123456789"),
            vec![SecretKind::ApiKey],
            "masked once, not twice"
        );
    }

    #[test]
    fn env_style_assignments_are_masked_whatever_the_case() {
        let r = redact("DATABASE_PASSWORD=\"s3cret-value\" make deploy");
        assert_eq!(r.text(), "DATABASE_PASSWORD=\"••••••••\" make deploy");
        assert_eq!(kinds("api_key: abcdefgh"), vec![SecretKind::Assignment]);
    }

    #[test]
    fn a_private_key_block_disappears_whole() {
        let r = redact(
            "before\n-----BEGIN OPENSSH PRIVATE KEY-----\nabc\ndef\n-----END OPENSSH PRIVATE KEY-----\nafter",
        );
        assert_eq!(r.text(), "before\n••••••••\nafter");
        assert_eq!(
            kinds("-----BEGIN RSA PRIVATE KEY-----\nx\n-----END RSA PRIVATE KEY-----"),
            vec![SecretKind::PrivateKey]
        );
    }

    #[test]
    fn ordinary_text_is_untouched() {
        for text in [
            "gti status",
            "npm run build",
            "the token of appreciation",
            "export PATH=$HOME/bin:$PATH",
            "sk-8",
        ] {
            let r = redact(text);
            assert_eq!(r.text(), text);
            assert!(r.findings().is_empty(), "{text}");
        }
    }
}

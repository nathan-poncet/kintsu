//! What a shell can run besides the programs on its PATH: the aliases and
//! functions its configuration defined, as the hook lists them once the
//! shell has finished starting.

/// An alias and what it stands for, as the shell defines it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alias {
    pub name: String,
    pub expansion: String,
}

/// Words that only change how the real command runs.
const RUN_PREFIXES: &[&str] = &[
    "command",
    "builtin",
    "exec",
    "noglob",
    "nocorrect",
    "sudo",
    "doas",
    "env",
    "nice",
    "time",
];

impl Alias {
    /// The program the alias runs: the first word of its expansion, past
    /// the words that only change how it runs (`command`, `exec`, `sudo`…).
    pub fn target(&self) -> Option<&str> {
        self.expansion
            .split_whitespace()
            .find(|word| !RUN_PREFIXES.contains(word) && !word.contains('='))
    }
}

/// The commands a shell knows beyond its PATH and builtins.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShellCommands {
    pub functions: Vec<String>,
    pub aliases: Vec<Alias>,
}

impl ShellCommands {
    /// More than this many entries, or a longer one, is not a shell's
    /// configuration: the rest is dropped.
    pub const MAX_ENTRIES: usize = 5000;
    pub const MAX_LENGTH: usize = 512;

    /// Keeps the lists within bounds.
    pub fn new(functions: Vec<String>, aliases: Vec<Alias>) -> Self {
        let mut names = Vec::new();
        let mut kept = Vec::new();
        for name in functions {
            if names.len() >= Self::MAX_ENTRIES {
                break;
            }
            if Self::is_plain_name(&name) {
                names.push(name);
            }
        }
        for alias in aliases {
            if names.len() + kept.len() >= Self::MAX_ENTRIES {
                break;
            }
            if Self::is_plain_name(&alias.name) && alias.expansion.len() <= Self::MAX_LENGTH {
                kept.push(alias);
            }
        }
        Self {
            functions: names,
            aliases: kept,
        }
    }

    /// One entry per line, as the hooks print them: `name` for a function,
    /// `name<TAB>expansion` for an alias. The quotes around an expansion
    /// are the shell's, not part of it. Names starting with `_` are the
    /// shell's own helpers and nobody's typo.
    pub fn parse(text: &str) -> Self {
        let mut functions = Vec::new();
        let mut aliases = Vec::new();
        for line in text.lines() {
            let line = line.trim_end_matches('\r');
            match line.split_once('\t') {
                Some((name, expansion)) => aliases.push(Alias {
                    name: name.trim().to_string(),
                    expansion: unquoted(expansion.trim()).to_string(),
                }),
                None if !line.trim().is_empty() => functions.push(line.trim().to_string()),
                None => {}
            }
        }
        Self::new(functions, aliases)
    }

    pub fn is_empty(&self) -> bool {
        self.functions.is_empty() && self.aliases.is_empty()
    }

    /// Every name the shell would run: functions and aliases.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.functions
            .iter()
            .map(String::as_str)
            .chain(self.aliases.iter().map(|a| a.name.as_str()))
    }

    fn is_plain_name(name: &str) -> bool {
        !name.is_empty()
            && name.len() <= Self::MAX_LENGTH
            && !name.starts_with('_')
            && !name.chars().any(char::is_whitespace)
    }
}

/// `'~/x'` and `"~/x"` are `~/x`.
fn unquoted(text: &str) -> &str {
    let bytes = text.as_bytes();
    if bytes.len() >= 2
        && (bytes[0] == b'\'' || bytes[0] == b'"')
        && bytes[bytes.len() - 1] == bytes[0]
    {
        &text[1..text.len() - 1]
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn functions_are_names_and_aliases_have_an_expansion_without_the_shells_quotes() {
        let parsed = ShellCommands::parse(
            "hmz\t'~/.dotnet/tools/hmz'\nll\t\"ls -l\"\ngst\tgit status\nfish_prompt\n__fish_helper\n_private\n\nf1\r\n",
        );
        assert_eq!(parsed.functions, vec!["fish_prompt", "f1"]);
        assert_eq!(
            parsed.aliases,
            vec![
                Alias {
                    name: "hmz".into(),
                    expansion: "~/.dotnet/tools/hmz".into()
                },
                Alias {
                    name: "ll".into(),
                    expansion: "ls -l".into()
                },
                Alias {
                    name: "gst".into(),
                    expansion: "git status".into()
                },
            ]
        );
        assert_eq!(
            parsed.names().collect::<Vec<_>>(),
            vec!["fish_prompt", "f1", "hmz", "ll", "gst"]
        );
        assert_eq!(parsed.aliases[1].target(), Some("ls"));
        assert!(ShellCommands::parse("  \n\n").is_empty());
    }

    #[test]
    fn the_target_is_the_program_past_the_words_that_only_change_how_it_runs() {
        let target = |e: &str| {
            Alias {
                name: "a".into(),
                expansion: e.into(),
            }
            .target()
            .map(str::to_string)
        };
        assert_eq!(target("command ls -G"), Some("ls".into()));
        assert_eq!(
            target("sudo systemctl restart nginx"),
            Some("systemctl".into())
        );
        assert_eq!(target("FOO=1 env BAR=2 ./run"), Some("./run".into()));
        assert_eq!(
            target("~/.dotnet/tools/hmz"),
            Some("~/.dotnet/tools/hmz".into())
        );
        assert_eq!(target(""), None);
    }

    #[test]
    fn the_lists_stay_within_bounds() {
        let many: Vec<String> = (0..6000).map(|i| format!("f{i}")).collect();
        let long = Alias {
            name: "big".into(),
            expansion: "x".repeat(ShellCommands::MAX_LENGTH + 1),
        };
        let bounded = ShellCommands::new(many, vec![long]);
        assert_eq!(bounded.functions.len(), ShellCommands::MAX_ENTRIES);
        assert!(
            bounded.aliases.is_empty(),
            "a longer expansion is not a shell's"
        );
        let spaced = ShellCommands::new(vec!["not a name".into()], vec![]);
        assert!(spaced.is_empty());
    }
}

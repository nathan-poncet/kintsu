//! How far apart two words are, for typo correction.

/// Damerau-Levenshtein distance (optimal string alignment): insertions,
/// deletions, substitutions and adjacent transpositions each cost one.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (n, m) = (a.len(), b.len());
    if n == 0 {
        return m;
    }
    if m == 0 {
        return n;
    }
    let mut d = vec![vec![0usize; m + 1]; n + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(d[i - 2][j - 2] + 1);
            }
            d[i][j] = best;
        }
    }
    d[n][m]
}

/// The candidate closest to `word` within `max`, when exactly one wins.
/// Among equally close candidates, one made of the same letters (`gti`
/// for `git`) beats one that is not (`gtr`). A remaining tie is refused:
/// guessing between `gut` and `git` would be a coin toss.
pub fn closest<'a>(
    word: &str,
    candidates: impl IntoIterator<Item = &'a str>,
    max: usize,
) -> Option<&'a str> {
    let letters = sorted_letters(word);
    let mut best: Option<(&str, (usize, bool))> = None;
    let mut tied = false;
    for candidate in candidates {
        if candidate == word {
            return Some(candidate);
        }
        let d = edit_distance(word, candidate);
        if d > max {
            continue;
        }
        let key = (d, sorted_letters(candidate) != letters);
        match best {
            Some((_, bk)) if key > bk => {}
            Some((_, bk)) if key == bk => tied = true,
            _ => {
                best = Some((candidate, key));
                tied = false;
            }
        }
    }
    match best {
        Some((c, _)) if !tied => Some(c),
        _ => None,
    }
}

fn sorted_letters(word: &str) -> Vec<char> {
    let mut letters: Vec<char> = word.chars().collect();
    letters.sort_unstable();
    letters
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_transposition_costs_one() {
        assert_eq!(edit_distance("gti", "git"), 1);
        assert_eq!(edit_distance("sl", "ls"), 1);
    }

    #[test]
    fn insertions_deletions_and_substitutions_cost_one_each() {
        assert_eq!(edit_distance("gi", "git"), 1);
        assert_eq!(edit_distance("gitt", "git"), 1);
        assert_eq!(edit_distance("gat", "git"), 1);
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(edit_distance("", "abc"), 3);
    }

    #[test]
    fn the_closest_candidate_wins_unless_two_tie() {
        assert_eq!(closest("gti", ["git", "go", "gcc"], 2), Some("git"));
        assert_eq!(closest("gut", ["git", "gat"], 1), None);
        assert_eq!(closest("xyzzy", ["git", "ls"], 2), None);
        assert_eq!(closest("git", ["git", "gut"], 2), Some("git"));
    }

    #[test]
    fn the_same_letters_in_another_order_break_a_tie() {
        assert_eq!(closest("gti", ["gtr", "git"], 1), Some("git"));
        assert_eq!(closest("sl", ["sh", "ls", "su", "nl"], 1), Some("ls"));
        assert_eq!(
            closest("tsl", ["tls", "stl"], 1),
            None,
            "two anagrams still tie"
        );
    }
}

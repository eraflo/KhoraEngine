// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Finds comments that tell the code's history instead of what holds.

use std::path::Path;

use super::allowlist::{add, Violations};
use super::comments::{comment_scanned_files, comments_of, runs, Comment, CommentKind};

/// Phrases that tell history wherever they stand, as lowercase words.
const HISTORY_PHRASES: &[&[&str]] = &[
    &["used", "to", "be"],
    &["formerly"],
    &["always", "was"],
    &["always", "were"],
    &["has", "always"],
    &["have", "always"],
    &["was", "renamed"],
    &["were", "renamed"],
    &["anymore"],
    &["these", "days"],
    &["before", "this", "change"],
    &["since", "the", "refactor"],
    &["old", "code"],
    &["new", "code"],
    &["until", "now"],
];

/// Subjects after which `used to` tells what the code did (`it used to
/// panic`), not what something serves for (`the key used to look it up`).
const USED_TO_SUBJECTS: &[&str] = &["it", "this", "that", "they", "we", "which", "who", "there"];

/// Verbs that tell a change when `now` comes right before them (`now
/// returns`, `now lives in`).
const NOW_VERBS: &[&str] = &["reads", "takes", "returns", "lives"];

/// Words after which `now`, followed by another word, tells a change (`is now
/// the default`); `as it is now.` tells the present and passes.
const NOW_AFTER: &[&str] = &["is", "are"];

/// Words after `the copies` that tell copies of code going apart.
const COPIES_PART: &[&str] = &[
    "disagree",
    "disagreed",
    "differ",
    "differed",
    "drift",
    "drifted",
    "diverge",
    "diverged",
];

/// A word of comment prose, or a barrier: code, a quotation or punctuation,
/// which no phrase crosses.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Token {
    Word { text: String, line: usize },
    Barrier,
}

/// Names each history phrase in the comments of `text`, at the line where it
/// starts; code in backticks and quoted text are not read.
pub fn history_phrases(text: &str) -> Vec<(usize, String)> {
    let comments = comments_of(text);
    let mut found: Vec<(usize, String)> = Vec::new();
    for run in runs(&comments) {
        let tokens = tokens_of(run);
        let words: Vec<Option<&str>> = (tokens.iter())
            .map(|token| match token {
                Token::Word { text, .. } => Some(text.as_str()),
                Token::Barrier => None,
            })
            .collect();
        let line_of = |at: usize| match &tokens[at] {
            Token::Word { line, .. } => *line,
            Token::Barrier => 0,
        };
        for at in 0..tokens.len() {
            if let Some((start, phrase)) = history_phrase_at(&words, at) {
                let entry = (
                    line_of(start),
                    format!("\"{phrase}\" tells history; state what holds"),
                );
                if !found.contains(&entry) {
                    found.push(entry);
                }
            }
        }
    }
    found
}

/// Collects, per file the comment checks read under `root`, each history
/// phrase ([`history_phrases`]), as `line N: …`.
pub fn history_violations(root: &Path) -> Violations {
    let mut violations = Violations::new();
    for (path, text) in comment_scanned_files(root) {
        for (line, message) in history_phrases(&text) {
            add(
                &mut violations,
                path.clone(),
                format!("line {line}: {message}"),
            );
        }
    }
    violations
}

/// Returns the history phrase read at word `at` of `words`, with the word
/// it starts at.
fn history_phrase_at(words: &[Option<&str>], at: usize) -> Option<(usize, String)> {
    let word = |k: usize| words.get(k).copied().flatten();
    let matches = |phrase: &[&str]| {
        (phrase.iter().enumerate()).all(|(k, expected)| word(at + k) == Some(expected))
    };
    if let Some(phrase) = HISTORY_PHRASES.iter().find(|phrase| matches(phrase)) {
        return Some((at, phrase.join(" ")));
    }
    let previous = at.checked_sub(1).and_then(word);
    let next = word(at + 1);
    // `no longer than` compares; every other `no longer` tells a change.
    if matches(&["no", "longer"]) && word(at + 2) != Some("than") {
        return Some((at, "no longer".to_owned()));
    }
    // `previously recorded` orders two runtime events; it tells no history.
    if word(at) == Some("previously") && !next.is_some_and(|w| w.ends_with("ed")) {
        return Some((at, "previously".to_owned()));
    }
    if matches(&["the", "copies"]) && word(at + 2).is_some_and(|w| COPIES_PART.contains(&w)) {
        return Some((at, "the copies".to_owned()));
    }
    if matches(&["used", "to"]) && previous.is_some_and(|w| USED_TO_SUBJECTS.contains(&w)) {
        return Some((at, "used to".to_owned()));
    }
    if word(at) == Some("now") {
        if let Some(before) = previous.filter(|w| NOW_AFTER.contains(w)) {
            if next.is_some() {
                return Some((at - 1, format!("{before} now")));
            }
        }
        if let Some(after) = next.filter(|w| NOW_VERBS.contains(w)) {
            return Some((at, format!("now {after}")));
        }
    }
    None
}

/// Splits a run of comment lines into words and barriers, the comment
/// markers left out.
fn tokens_of(run: &[Comment]) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut quote: Option<char> = None;
    for comment in run {
        let mut word = String::new();
        let flush = |word: &mut String, tokens: &mut Vec<Token>| {
            let text = word.trim_end_matches('\'').to_ascii_lowercase();
            if !text.is_empty() {
                tokens.push(Token::Word {
                    text,
                    line: comment.line,
                });
            }
            word.clear();
        };
        for ch in prose(comment).chars() {
            if let Some(close) = quote {
                if ch == close {
                    quote = None;
                }
                continue;
            }
            match ch {
                '`' | '"' | '\u{201c}' => {
                    flush(&mut word, &mut tokens);
                    tokens.push(Token::Barrier);
                    quote = Some(if ch == '\u{201c}' { '\u{201d}' } else { ch });
                }
                _ if ch.is_alphanumeric() || ch == '_' || (ch == '\'' && !word.is_empty()) => {
                    word.push(ch);
                }
                _ if ch.is_whitespace() => flush(&mut word, &mut tokens),
                _ => {
                    flush(&mut word, &mut tokens);
                    tokens.push(Token::Barrier);
                }
            }
        }
        flush(&mut word, &mut tokens);
    }
    tokens
}

/// Returns the prose of one comment line: its text past the comment markers.
fn prose(comment: &Comment) -> &str {
    let text = comment.text.as_str();
    match comment.kind {
        CommentKind::Doc => text.get(3..).unwrap_or(""),
        CommentKind::Inner => text.get(3..).unwrap_or(""),
        CommentKind::Line => text.trim_start_matches('/'),
        CommentKind::Block => {
            let text = text.trim_start();
            let text = text.strip_prefix("/*").unwrap_or(text);
            let text = text.trim_start_matches(['*', '!']);
            text.strip_suffix("*/").unwrap_or(text)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_history_phrase_is_named_at_its_line() {
        let text = "/// Returns the id.\n///\n/// This no longer locks.\nfn f() {}\n";
        assert_eq!(
            history_phrases(text),
            vec![(
                3,
                "\"no longer\" tells history; state what holds".to_owned()
            )]
        );
    }

    #[test]
    fn a_phrase_wrapped_across_comment_lines_is_named() {
        let text = "// The slot is\n// now freed by the caller.\n// It now returns the id.\n";
        let lines: Vec<usize> = history_phrases(text)
            .into_iter()
            .map(|(line, _)| line)
            .collect();
        assert_eq!(lines, vec![1, 3]);
    }

    #[test]
    fn a_phrase_in_backticks_or_quotes_or_code_passes() {
        let text = "/// Renames with `#[component(formerly = \"Old\")]`.\n\
                    /// Rejects \"used to be\" in comments.\n\
                    let s = \"no longer\";\n";
        assert!(history_phrases(text).is_empty());
    }

    #[test]
    fn a_comparison_an_ordering_and_the_present_pass() {
        let text = "/// Zero when no longer than `EPSILON`.\n\
                    /// Submits a previously recorded buffer.\n\
                    /// Reads the world as it is now.\n\
                    /// Refreshes the copies the engine holds.\n";
        assert!(
            history_phrases(text).is_empty(),
            "{:?}",
            history_phrases(text)
        );
    }

    #[test]
    fn used_to_naming_a_purpose_passes_and_a_past_habit_is_named() {
        let purpose = "/// The key used to look the slot up.\n";
        assert!(history_phrases(purpose).is_empty());
        let habit = "// This used to panic on an empty list.\n";
        assert_eq!(history_phrases(habit).len(), 1);
    }
}

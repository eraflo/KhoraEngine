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

//! Checks that a function's doc summary opens on a verb in the third person.

use std::ops::Range;
use std::path::Path;

use super::allowlist::Violations;
use super::code::{
    blank_literals, closing_bracket, is_ident_char, leading_ident, strip_rust_comments,
};
use super::comments::{comments_of, per_source_violations, runs, Comment, CommentKind};
use super::functions::cfg_test_item_ranges;

/// Verbs a summary may open on that the ending rule alone would not tell.
const VERBS: &[&str] = &["Is", "Has", "Does"];

/// Words ending in `s` that are no verb in the third person.
const NOT_VERBS: &[&str] = &[
    "This",
    "Its",
    "Was",
    "Thus",
    "Plus",
    "Always",
    "Unless",
    "Yes",
    "As",
    "Us",
    "Perhaps",
    "Whereas",
    "Sometimes",
    "Besides",
    "Afterwards",
    "Towards",
    "Nevertheless",
];

/// Words that may stand before `fn` in a function's header.
const FN_QUALIFIERS: &[&str] = &[
    "pub", "const", "async", "unsafe", "safe", "extern", "default",
];

/// Names each function of `text` whose `///` summary does not open on a verb
/// in the third person (`Returns`, `Builds`, `Is`).
///
/// Methods of a trait impl (documented by the trait), `#[test]` functions and
/// everything under `#[cfg(test)]` are not read.
pub fn summaries_without_a_verb(text: &str) -> Vec<(usize, String)> {
    let code: Vec<char> = blank_literals(&strip_rust_comments(text)).chars().collect();
    let line_starts = line_starts(&code);
    let tests = cfg_test_item_ranges(&code);
    let trait_impls = trait_impl_bodies(&code);
    let docs: Vec<Comment> = comments_of(text)
        .into_iter()
        .filter(|comment| comment.kind == CommentKind::Doc && comment.whole_line)
        .collect();
    let mut found = Vec::new();
    for block in runs(&docs) {
        let Some(last) = block.last() else {
            continue;
        };
        let Some(&start) = line_starts.get(last.line) else {
            continue;
        };
        let Some(at) = documented_fn(&code, start) else {
            continue;
        };
        let skipped = |ranges: &[Range<usize>]| ranges.iter().any(|range| range.contains(&at));
        if skipped(&tests) || skipped(&trait_impls) {
            continue;
        }
        let Some(summary) = block
            .iter()
            .find(|comment| !comment.text[3..].trim().is_empty())
        else {
            continue;
        };
        let word = summary.text[3..]
            .split_whitespace()
            .next()
            .unwrap_or_default();
        if !is_third_person_verb(word) {
            let message = format!(
                "summary opens on `{word}`, not a verb in the third person (\"Returns…\", \
                 \"Builds…\")"
            );
            found.push((summary.line, message));
        }
    }
    found
}

/// Collects, per non-test file of `root`'s `src` trees, each function summary
/// not opening on a verb ([`summaries_without_a_verb`]), as `line N: …`.
pub fn summary_verb_violations(root: &Path) -> Violations {
    per_source_violations(root, summaries_without_a_verb)
}

/// Tells whether `word` reads as a verb in the third person: a capitalised
/// word ending in a single `s`, or one of `VERBS`.
pub fn is_third_person_verb(word: &str) -> bool {
    let word = word.trim_end_matches([',', ':', ';', '.']);
    if VERBS.contains(&word) {
        return true;
    }
    word.chars().next().is_some_and(char::is_uppercase)
        && word.chars().all(char::is_alphabetic)
        && word.ends_with('s')
        && !word.ends_with("ss")
        && !NOT_VERBS.contains(&word)
}

/// Returns the character offset at which each line of `code` starts, the
/// first line at index 0.
fn line_starts(code: &[char]) -> Vec<usize> {
    let mut starts = vec![0];
    starts.extend(
        (code.iter().enumerate())
            .filter(|(_, ch)| **ch == '\n')
            .map(|(at, _)| at + 1),
    );
    starts
}

/// Returns where the `fn` keyword stands when the item starting at `from`
/// (attributes aside) is a function.
///
/// `None` for any other item, and for a `#[test]` function.
fn documented_fn(code: &[char], from: usize) -> Option<usize> {
    let mut at = from;
    let skip_space = |at: &mut usize| {
        while code.get(*at).is_some_and(|ch| ch.is_whitespace()) {
            *at += 1;
        }
    };
    loop {
        skip_space(&mut at);
        if code.get(at) != Some(&'#') || code.get(at + 1) != Some(&'[') {
            break;
        }
        let end = closing_bracket(code, at + 1);
        let attr: String = code[at..end]
            .iter()
            .filter(|ch| !ch.is_whitespace())
            .collect();
        if attr == "#[test]" {
            return None;
        }
        at = end;
    }
    loop {
        skip_space(&mut at);
        let rest: String = code[at..code.len().min(at + 64)].iter().collect();
        let word = leading_ident(&rest);
        if word == "fn" {
            return Some(at);
        }
        if !FN_QUALIFIERS.contains(&word) {
            return None;
        }
        at += word.chars().count();
        skip_space(&mut at);
        // `pub(crate)` and `extern "C"` carry a group after their word.
        let close = match code.get(at) {
            Some('(') => Some(')'),
            Some('"') => Some('"'),
            _ => None,
        };
        if let Some(close) = close {
            at += 1 + code[at + 1..].iter().position(|&ch| ch == close)? + 1;
        }
    }
}

/// Returns the character ranges of the bodies of every `impl Trait for Type`
/// block in `code`, comments and literals blanked.
fn trait_impl_bodies(code: &[char]) -> Vec<Range<usize>> {
    let mut bodies = Vec::new();
    let mut header_start = 0;
    for (at, &ch) in code.iter().enumerate() {
        match ch {
            '{' => {
                let header: String = code[header_start..at].iter().collect();
                if is_trait_impl_header(&header) {
                    bodies.push(at..closing_brace(code, at));
                }
                header_start = at + 1;
            }
            ';' | '}' => header_start = at + 1,
            _ => {}
        }
    }
    bodies
}

/// Tells whether an item header (the text before its `{`) opens a trait
/// impl: `impl<T> Trait for Type`, attributes and `unsafe` allowed.
fn is_trait_impl_header(header: &str) -> bool {
    let mut rest = header.trim_start();
    while rest.starts_with("#[") {
        let chars: Vec<char> = rest.chars().collect();
        let end = closing_bracket(&chars, 1);
        rest = rest[chars[..end].iter().map(|ch| ch.len_utf8()).sum::<usize>()..].trim_start();
    }
    let rest = rest.strip_prefix("unsafe").map_or(rest, str::trim_start);
    let Some(after) = rest.strip_prefix("impl") else {
        return false;
    };
    if after.starts_with(is_ident_char) {
        return false;
    }
    let header = after.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut depth = 0i32;
    for (at, ch) in header.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' if !header[..at].ends_with('-') => depth -= 1,
            _ => {}
        }
        if depth == 0 && header[at..].starts_with(" where ") {
            return false;
        }
        if depth == 0 && header[at..].starts_with(" for ") {
            return true;
        }
    }
    false
}

/// Returns the index of the `}` closing the `{` at `open`, or the end.
fn closing_brace(code: &[char], open: usize) -> usize {
    let mut depth = 0usize;
    for (at, &ch) in code.iter().enumerate().skip(open) {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return at;
                }
            }
            _ => {}
        }
    }
    code.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_summary_opening_on_a_noun_is_named() {
        let text = "/// Whether the slot is free.\npub fn is_free() -> bool {\n    true\n}\n";
        let found = summaries_without_a_verb(text);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].0, 1);
        assert!(found[0].1.contains("`Whether`"), "{found:?}");
    }

    #[test]
    fn a_summary_opening_on_a_verb_passes() {
        let text = "/// Returns the id.\n#[inline]\npub(crate) const fn id() -> u8 {\n    0\n}\n\
                    /// Is the slot free.\nfn free() {}\n";
        assert!(summaries_without_a_verb(text).is_empty());
    }

    #[test]
    fn trait_impl_methods_tests_and_other_items_pass() {
        let text = "/// The id of a slot.\npub struct Id;\n\
                    impl Default for Id {\n    /// The default id.\n    fn default() -> Self {\n        Id\n    }\n}\n\
                    /// The test.\n#[test]\nfn pins() {}\n\
                    #[cfg(test)]\nmod tests {\n    /// A helper.\n    fn helper() {}\n}\n";
        assert!(
            summaries_without_a_verb(text).is_empty(),
            "{:?}",
            summaries_without_a_verb(text)
        );
    }

    #[test]
    fn an_inherent_method_summary_is_read() {
        let text = "pub struct Id;\nimpl Id {\n    /// The raw value.\n    pub fn raw(&self) -> u8 {\n        0\n    }\n}\n";
        assert_eq!(summaries_without_a_verb(text).len(), 1);
    }

    #[test]
    fn the_verb_rule_takes_third_person_words_only() {
        for verb in ["Returns", "Builds", "Is", "Has", "Does", "Tells:"] {
            assert!(is_third_person_verb(verb), "{verb}");
        }
        for word in [
            "Whether", "Process", "This", "The", "`fn`", "Engine's", "returns",
        ] {
            assert!(!is_third_person_verb(word), "{word}");
        }
    }
}

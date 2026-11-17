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

//! Tells Rust code from its comments and literals, line numbers kept.

use std::collections::BTreeSet;

/// Tells whether the `r` at `at` opens a raw string: it follows no identifier
/// character, or only the `b` of a byte raw string (`br"…"`).
fn opens_raw_string(chars: &[char], at: usize) -> bool {
    let ident = |at: usize| chars.get(at).is_some_and(|&ch| is_ident_char(ch));
    match at.checked_sub(1) {
        None => true,
        Some(before) if chars[before] == 'b' => before == 0 || !ident(before - 1),
        Some(before) => !ident(before),
    }
}

/// Returns `text` with every `//` and `/* */` comment blanked, docs included.
///
/// String and character literals and every newline are kept, so line
/// numbers survive.
pub fn strip_rust_comments(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let blank = |c: char| if c == '\n' { '\n' } else { ' ' };
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c == '/' && next == Some('/') {
            while i < chars.len() && chars[i] != '\n' {
                out.push(' ');
                i += 1;
            }
        } else if c == '/' && next == Some('*') {
            let mut depth = 0usize;
            while i < chars.len() {
                if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                    depth += 1;
                    out.push_str("  ");
                    i += 2;
                } else if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    out.push_str("  ");
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    out.push(blank(chars[i]));
                    i += 1;
                }
            }
        } else if c == '"' {
            out.push(c);
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    out.push(chars[i]);
                    i += 1;
                }
                out.push(chars[i]);
                i += 1;
            }
            if i < chars.len() {
                out.push('"');
                i += 1;
            }
        } else if c == 'r'
            && (next == Some('"') || next == Some('#'))
            && opens_raw_string(&chars, i)
        {
            // A raw string: r"…", r#"…"#, br"…", …
            let mut j = i + 1;
            let mut hashes = 0;
            while chars.get(j) == Some(&'#') {
                hashes += 1;
                j += 1;
            }
            if chars.get(j) != Some(&'"') {
                out.push(c);
                i += 1;
                continue;
            }
            let closing: String = std::iter::once('"')
                .chain(std::iter::repeat_n('#', hashes))
                .collect();
            let body_start = j + 1;
            let rest: String = chars[body_start..].iter().collect();
            let end = rest.find(&closing).map_or(chars.len(), |at| {
                body_start + rest[..at].chars().count() + closing.chars().count()
            });
            out.extend(chars[i..end].iter());
            i = end;
        } else if c == '\'' {
            // A character literal ('x', '\n', '\''); otherwise a lifetime.
            let literal_end = match (next, chars.get(i + 2), chars.get(i + 3)) {
                // The escaped character sits at `i + 2`; the closing quote follows.
                (Some('\\'), _, _) => chars
                    .get(i + 3..)
                    .and_then(|rest| rest.iter().position(|&ch| ch == '\''))
                    .map(|at| i + 3 + at + 1),
                (Some(_), Some('\''), _) => Some(i + 3),
                _ => None,
            };
            match literal_end {
                Some(end) if end <= chars.len() => {
                    out.extend(chars[i..end].iter());
                    i = end;
                }
                _ => {
                    out.push(c);
                    i += 1;
                }
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// Tells whether `c` may stand in an identifier.
pub fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Tells whether `line` holds `word` as a whole identifier.
pub fn names_word(line: &str, word: &str) -> bool {
    line.match_indices(word).any(|(at, _)| {
        let before = line[..at].chars().next_back();
        let after = line[at + word.len()..].chars().next();
        !before.is_some_and(is_ident_char) && !after.is_some_and(is_ident_char)
    })
}

/// Returns the 1-based lines of `code` that read the `runtime` field of a
/// binding holding an `EngineContext`.
pub fn runtime_field_reads(code: &str) -> Vec<usize> {
    let mut bindings = BTreeSet::new();
    for (at, _) in code.match_indices("EngineContext") {
        let before = code[..at].trim_end();
        // `name: &mut EngineContext` / `name: &EngineContext`
        let typed = before
            .strip_suffix("&mut")
            .or_else(|| before.strip_suffix('&'))
            .map(str::trim_end)
            .and_then(|b| b.strip_suffix(':'));
        // `let [mut] name = EngineContext::…`
        let built = before.strip_suffix('=');
        if let Some(prefix) = typed.or(built) {
            let name: String = prefix
                .trim_end()
                .chars()
                .rev()
                .take_while(|c| is_ident_char(*c))
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            if !name.is_empty() && name != "self" {
                bindings.insert(name);
            }
        }
    }

    let mut lines = BTreeSet::new();
    for (index, line) in code.lines().enumerate() {
        for name in &bindings {
            let access = format!("{name}.runtime");
            let reads = line.match_indices(&access).any(|(at, _)| {
                let before = line[..at].chars().next_back();
                let after = line[at + access.len()..].chars().next();
                !before.is_some_and(|c| is_ident_char(c) || c == '.')
                    && !after.is_some_and(|c| is_ident_char(c) || c == '(')
            });
            if reads {
                lines.insert(index + 1);
            }
        }
    }
    lines.into_iter().collect()
}

/// Returns the index just past the `]` closing the `[` at `open`, or the end.
pub fn closing_bracket(chars: &[char], open: usize) -> usize {
    let mut depth = 0usize;
    for (at, &ch) in chars.iter().enumerate().skip(open) {
        match ch {
            '[' => depth += 1,
            ']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return at + 1;
                }
            }
            _ => {}
        }
    }
    chars.len()
}

/// Returns `code` (comments already blanked) with the contents of every
/// string and character literal blanked.
///
/// Reads `"…"`, `r#"…"#`, `b"…"` and `br"…"`; quotes and newlines are kept,
/// so line numbers survive.
pub fn blank_literals(code: &str) -> String {
    let chars: Vec<char> = code.chars().collect();
    let mut out = String::with_capacity(code.len());
    let blank = |ch: char| if ch == '\n' { '\n' } else { ' ' };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let after_ident = i > 0 && is_ident_char(chars[i - 1]);
        let raw_quote_search = match (c, chars.get(i + 1)) {
            ('r', _) if !after_ident => Some(i + 1),
            ('b', Some('r')) if !after_ident => Some(i + 2),
            _ => None,
        };
        if let Some(mut j) = raw_quote_search {
            let mut hashes = 0;
            while chars.get(j) == Some(&'#') {
                hashes += 1;
                j += 1;
            }
            if chars.get(j) == Some(&'"') {
                out.extend(&chars[i..=j]);
                let mut k = j + 1;
                while k < chars.len()
                    && !(chars[k] == '"' && (1..=hashes).all(|h| chars.get(k + h) == Some(&'#')))
                {
                    out.push(blank(chars[k]));
                    k += 1;
                }
                let end = (k + 1 + hashes).min(chars.len());
                out.extend(&chars[k.min(chars.len())..end]);
                i = end;
                continue;
            }
        }
        if c == '"' {
            out.push('"');
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    out.push(' ');
                    out.push(blank(chars[i + 1]));
                    i += 2;
                    continue;
                }
                out.push(blank(chars[i]));
                i += 1;
            }
            if i < chars.len() {
                out.push('"');
                i += 1;
            }
            continue;
        }
        if c == '\'' {
            // A character literal ('x', '\n', '\''); otherwise a lifetime.
            let end = match (chars.get(i + 1), chars.get(i + 2)) {
                (Some('\\'), _) => chars
                    .get(i + 3..)
                    .and_then(|rest| rest.iter().position(|&ch| ch == '\''))
                    .map(|at| i + 3 + at + 1),
                (Some(_), Some('\'')) => Some(i + 3),
                _ => None,
            };
            if let Some(end) = end.filter(|&end| end <= chars.len()) {
                out.push('\'');
                out.extend(std::iter::repeat_n(' ', end - i - 2));
                out.push('\'');
                i = end;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Returns the leading identifier of `text` (empty when it starts otherwise).
pub fn leading_ident(text: &str) -> &str {
    let len = text
        .char_indices()
        .find(|&(_, ch)| !is_ident_char(ch))
        .map_or(text.len(), |(at, _)| at);
    &text[..len]
}

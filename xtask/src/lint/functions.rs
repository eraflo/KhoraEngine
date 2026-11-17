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

//! Measures every function with a body, for the function length check.

use std::path::Path;

use super::allowlist::{add, Violations};
use super::code::{blank_literals, closing_bracket, is_ident_char, strip_rust_comments};
use super::modules::{is_cfg_test, non_test_rust_files};
use super::tree::{rel, rust_src_roots};

/// Largest number of lines a non-test function may span, from its `fn`
/// keyword to the brace closing its body.
pub const MAX_FN_LINES: usize = 200;

/// A function with a body, as the length check reads it.
#[derive(Debug, PartialEq, Eq)]
pub struct FnSpan {
    /// 1-based line of the `fn` keyword.
    pub line: usize,
    /// The function's name.
    pub name: String,
    /// Lines from the `fn` keyword to the brace closing the body, both
    /// counted.
    pub lines: usize,
}

/// Measures every function with a body in `text`, in order, at any depth.
///
/// Reads free functions, methods of `impl` and `trait` blocks, functions
/// nested in others. Comments and literal contents are blanked first. Not read: a
/// function without a body (`fn f();` in a trait), a function type
/// (`fn(u8) -> u8`), a name a macro builds (`fn $name`, `fn #name`), and
/// anything inside an item under `#[cfg(test)]` (an inline test module, a
/// test helper, a test-only `impl`).
pub fn function_spans(text: &str) -> Vec<FnSpan> {
    let code = blank_literals(&strip_rust_comments(text));
    let chars: Vec<char> = code.chars().collect();
    let mut line_at = Vec::with_capacity(chars.len());
    let mut line = 1usize;
    for &ch in &chars {
        line_at.push(line);
        if ch == '\n' {
            line += 1;
        }
    }
    let tests = cfg_test_item_ranges(&chars);
    let in_tests = |at: usize| tests.iter().any(|range| range.contains(&at));
    let mut spans = Vec::new();
    let mut i = 0usize;
    while i + 2 < chars.len() {
        let keyword = chars[i] == 'f'
            && chars[i + 1] == 'n'
            && chars[i + 2].is_whitespace()
            && (i == 0 || !is_ident_char(chars[i - 1]));
        if !keyword || in_tests(i) {
            i += 1;
            continue;
        }
        let mut at = i + 2;
        while chars.get(at).is_some_and(|ch| ch.is_whitespace()) {
            at += 1;
        }
        let name: String = chars[at..]
            .iter()
            .take_while(|&&ch| is_ident_char(ch))
            .collect();
        if !name.starts_with(|ch: char| ch.is_alphabetic() || ch == '_') {
            i += 2;
            continue;
        }
        at += name.chars().count();
        // The body opens at the first `{` outside parentheses and brackets;
        // a `;` there first means no body.
        let mut depth = 0usize;
        let mut open = None;
        for (j, &ch) in chars.iter().enumerate().skip(at) {
            match ch {
                '(' | '[' => depth += 1,
                ')' | ']' => depth = depth.saturating_sub(1),
                '{' if depth == 0 => {
                    open = Some(j);
                    break;
                }
                ';' if depth == 0 => break,
                _ => {}
            }
        }
        if let Some(open) = open {
            let mut braces = 0usize;
            let mut close = chars.len() - 1;
            for (j, &ch) in chars.iter().enumerate().skip(open) {
                match ch {
                    '{' => braces += 1,
                    '}' => {
                        braces -= 1;
                        if braces == 0 {
                            close = j;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            spans.push(FnSpan {
                line: line_at[i],
                name,
                lines: line_at[close] - line_at[i] + 1,
            });
        }
        i = at;
    }
    spans
}

/// Returns the character ranges of `chars` (comments and literals blanked)
/// held by an item under `#[cfg(test)]` or `#[cfg(all(test, …))]`.
///
/// Items at any depth count, each from its attribute to the `;` ending it or
/// the brace closing its body.
pub fn cfg_test_item_ranges(chars: &[char]) -> Vec<std::ops::Range<usize>> {
    let mut ranges = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] != '#' || chars.get(i + 1) != Some(&'[') {
            i += 1;
            continue;
        }
        let attr_end = closing_bracket(chars, i + 1);
        let attr: String = chars[i..attr_end].iter().collect();
        if !is_cfg_test(&attr) {
            i = attr_end;
            continue;
        }
        let mut depth = 0usize;
        let mut end = chars.len();
        for (j, &ch) in chars.iter().enumerate().skip(attr_end) {
            match ch {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 && ch == '}' {
                        end = j + 1;
                        break;
                    }
                }
                ';' if depth == 0 => {
                    end = j + 1;
                    break;
                }
                _ => {}
            }
        }
        ranges.push(i..end);
        i = end;
    }
    ranges
}

/// Collects, per non-test file under `root`, each function spanning more than
/// `max` lines, keyed `<path>::<name>`.
pub fn long_function_violations(root: &Path, max: usize) -> Violations {
    let mut violations = Violations::new();
    for src in rust_src_roots(root) {
        for (path, text) in non_test_rust_files(&src) {
            let path = rel(root, &path);
            for span in function_spans(&text)
                .into_iter()
                .filter(|span| span.lines > max)
            {
                add(
                    &mut violations,
                    format!("{path}::{}", span.name),
                    format!(
                        "line {}: `fn {}` spans {} lines",
                        span.line, span.name, span.lines
                    ),
                );
            }
        }
    }
    violations
}

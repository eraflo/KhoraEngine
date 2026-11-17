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

//! Reads a file's comments, and checks what they say and how long they run.

use std::path::Path;

use super::allowlist::{add, Violations};
use super::code::{is_ident_char, strip_rust_comments};
use super::modules::non_test_rust_files;
use super::tree::{child_dirs, has_extension, read_text, rel, rust_src_roots, walk_files};

/// What a comment is: a line comment, an item doc, a module doc, or a block
/// comment (or a line inside one).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommentKind {
    /// A `//` comment.
    Line,
    /// A `///` item doc.
    Doc,
    /// A `//!` module doc.
    Inner,
    /// A `/* */` comment, or a line inside one.
    Block,
}

/// One line's comment, read from a file.
#[derive(Clone, Debug)]
pub struct Comment {
    /// 1-based line.
    pub line: usize,
    /// What the comment is.
    pub kind: CommentKind,
    /// From the comment's opening `//` or `/*` (inside a block comment, from
    /// the line's first character) to the end of the line.
    pub text: String,
    /// Nothing but whitespace before the comment on its line.
    pub whole_line: bool,
    /// The line starts with whitespace.
    pub indented: bool,
}

/// Reads the comment of each line of `text` that holds one, told from code
/// by [`strip_rust_comments`], which blanks comments and keeps literals.
pub fn comments_of(text: &str) -> Vec<Comment> {
    let stripped = strip_rust_comments(text);
    let mut comments = Vec::new();
    let mut in_block = false;
    for (index, (raw, code)) in text.lines().zip(stripped.lines()).enumerate() {
        let raw: Vec<char> = raw.chars().collect();
        let code: Vec<char> = code.chars().collect();
        let Some(start) = (0..raw.len()).find(|&at| code.get(at) != Some(&raw[at])) else {
            // A blank line inside a block comment keeps it open.
            in_block = in_block && raw.iter().all(|ch| ch.is_whitespace());
            continue;
        };
        let comment: String = raw[start..].iter().collect();
        let first = raw.iter().position(|ch| !ch.is_whitespace()).unwrap_or(0);
        let kind = if in_block && start == first {
            CommentKind::Block
        } else if comment.starts_with("///") && !comment.starts_with("////") {
            CommentKind::Doc
        } else if comment.starts_with("//!") {
            CommentKind::Inner
        } else if comment.starts_with("//") {
            CommentKind::Line
        } else {
            CommentKind::Block
        };
        in_block = block_open_after(&raw, &code, start, in_block && start == first);
        comments.push(Comment {
            line: index + 1,
            kind,
            text: comment,
            whole_line: start == first,
            indented: raw.first().is_some_and(|ch| ch.is_whitespace()),
        });
    }
    comments
}

/// Tells whether a block comment is still open at the end of a line.
///
/// Reads its `raw` and comment-blanked `code` characters from `start` on:
/// `/*` and `*/` are counted where they are comment, nesting kept, until a
/// `//` outside any block ends the line. `open` says the line starts inside one.
fn block_open_after(raw: &[char], code: &[char], start: usize, open: bool) -> bool {
    let comment = |at: usize| code.get(at) != raw.get(at);
    let mut depth = usize::from(open);
    let mut at = start;
    while at + 1 < raw.len() {
        let pair = (raw[at], raw[at + 1]);
        if comment(at) && comment(at + 1) {
            match pair {
                ('/', '*') => {
                    depth += 1;
                    at += 2;
                    continue;
                }
                ('*', '/') if depth > 0 => {
                    depth -= 1;
                    at += 2;
                    continue;
                }
                ('/', '/') if depth == 0 => return false,
                _ => {}
            }
        }
        at += 1;
    }
    depth > 0
}

/// Cuts `comments` into runs of consecutive lines.
pub fn runs(comments: &[Comment]) -> Vec<&[Comment]> {
    let mut runs = Vec::new();
    let mut start = 0;
    for at in 1..=comments.len() {
        if at == comments.len() || comments[at].line != comments[at - 1].line + 1 {
            if start < at {
                runs.push(&comments[start..at]);
            }
            start = at;
        }
    }
    runs
}

/// Lists every `.rs` file the comment checks read, with its repo-relative
/// path and its text.
///
/// Reads `crates/*/src`, `crates/*/tests`, `xtask/src`, `xtask/tests` and
/// `examples/*/src`, test files included.
pub fn comment_scanned_files(root: &Path) -> Vec<(String, String)> {
    let mut trees = rust_src_roots(root);
    for member in child_dirs(&root.join("crates")) {
        trees.push(member.join("tests"));
    }
    trees.push(root.join("xtask/tests"));
    let mut files = Vec::new();
    for tree in trees.into_iter().filter(|tree| tree.is_dir()) {
        for path in walk_files(&tree) {
            if has_extension(&path, "rs") {
                files.push((rel(root, &path), read_text(&path)));
            }
        }
    }
    files.sort();
    files
}

/// Returns `comment` with the quoted heading of each book citation blanked:
/// a book heading that numbers its steps is its text, not a plan label.
fn without_cited_headings(comment: &str) -> String {
    let mut out = comment.to_owned();
    for (open, close) in cited_heading_spans(comment) {
        out.replace_range(open..close, &" ".repeat(close - open));
    }
    out
}

/// Returns the byte span of each heading `comment` cites, quotes aside: the
/// quoted text right after a cited page, as [`book_citations`] reads it.
fn cited_heading_spans(comment: &str) -> Vec<(usize, usize)> {
    let path_len = |text: &str| -> Option<usize> {
        let path: String = text
            .chars()
            .take_while(|&ch| ch.is_alphanumeric() || matches!(ch, '/' | '-' | '_' | '.'))
            .collect();
        path.find(".md").map(|end| end + 3)
    };
    let heading_after = |from: usize| -> Option<(usize, usize)> {
        let rest = &comment[from..];
        let ticks = rest.len() - rest.trim_start_matches('`').len();
        let open = from + ticks + rest[ticks..].strip_prefix(", \"").map(|_| 3)?;
        let close = open + comment[open..].find('"')?;
        Some((open, close))
    };
    let mut spans = Vec::new();
    for (at, _) in comment.match_indices("docs/src/") {
        if let Some(len) = path_len(&comment[at..]) {
            spans.extend(heading_after(at + len));
        }
    }
    for (at, marker) in comment.match_indices("See the book:") {
        let after = at + marker.len();
        let start = after + (comment[after..].len() - comment[after..].trim_start().len());
        if let Some(len) = path_len(&comment[start..]) {
            spans.extend(heading_after(start + len));
        }
    }
    spans
}

/// Tells whether `comment` names a plan or review label: a phase, a step or
/// a review round with its number or letter, a review tag, a spec by number.
pub fn names_plan_label(comment: &str) -> bool {
    let chars: Vec<char> = comment.chars().collect();
    let boundary = |at: usize| at == 0 || !is_ident_char(chars[at - 1]);
    let digit = |at: usize| chars.get(at).is_some_and(char::is_ascii_digit);
    let word = |at: usize, word: &str| {
        word.chars()
            .enumerate()
            .all(|(k, ch)| chars.get(at + k) == Some(&ch))
    };
    let digits_end = |mut at: usize| {
        while digit(at) {
            at += 1;
        }
        at
    };
    (0..chars.len()).any(|at| {
        let labelled = |name: &str| word(at, name) && boundary(at) && digit(at + name.len());
        if labelled("Phase ") || labelled("Round ") || labelled("round ") {
            return true;
        }
        if labelled("Steps ") || labelled("spec ") || labelled("Spec ") {
            return true;
        }
        // A phase named by a capital letter.
        if word(at, "Phase ")
            && boundary(at)
            && chars.get(at + 6).is_some_and(char::is_ascii_uppercase)
            && !chars.get(at + 7).is_some_and(|&ch| is_ident_char(ch))
        {
            return true;
        }
        if labelled("Step ") || labelled("step ") {
            let mut end = digits_end(at + 5);
            if chars.get(end).is_some_and(char::is_ascii_lowercase) {
                end += 1;
            }
            return !chars.get(end).is_some_and(|&ch| is_ident_char(ch));
        }
        if chars[at] == 'R' && boundary(at) && digit(at + 1) {
            return chars.get(digits_end(at + 1)) == Some(&':');
        }
        if word(at, "faute ") {
            let after = at + 6;
            return digit(after) || (chars.get(after) == Some(&'#') && digit(after + 1));
        }
        word(at, "ergon-E") && digit(at + 7)
    })
}

/// Tells whether a `//` comment opens on a numbered step: `// 1. `,
/// `// 2b. `, `// 2b-bis. `, `// 3) `, `// 6–8. `, `// (1) `.
pub fn opens_on_numbered_step(comment: &str) -> bool {
    let rest = comment.trim_start_matches('/').trim_start();
    let number = |text: &str| -> Option<usize> {
        let digits = text.chars().take_while(char::is_ascii_digit).count();
        (digits > 0).then(|| {
            let letter = text[digits..].starts_with(|ch: char| ch.is_ascii_lowercase());
            digits + usize::from(letter)
        })
    };
    if let Some(inner) = rest.strip_prefix('(') {
        return number(inner).is_some_and(|end| {
            (inner[end..].strip_prefix(')'))
                .is_some_and(|after| after.starts_with(char::is_whitespace))
        });
    }
    let Some(mut end) = number(rest) else {
        return false;
    };
    let mut tail = &rest[end..];
    tail = tail.strip_prefix("-bis").unwrap_or(tail);
    // A range of steps: `6–8.`, `6-8.`.
    if let Some(range) = tail.strip_prefix('–').or_else(|| tail.strip_prefix('-')) {
        if let Some(second) = number(range) {
            end = second;
            tail = &range[end..];
        }
    }
    let Some(after) = tail.strip_prefix('.').or_else(|| tail.strip_prefix(')')) else {
        return false;
    };
    after.starts_with(char::is_whitespace)
}

/// Lists the book pages `comment` cites, each with the heading it names and
/// where the citation starts.
///
/// A citation is a path `docs/src/….md`, or `See the book: <path under
/// docs/src>.md`, either followed by `, "Heading"`. Paths are repo-relative.
pub fn book_citations(comment: &str) -> Vec<(String, Option<String>, usize)> {
    let path_at = |text: &str| -> Option<String> {
        let path: String = text
            .chars()
            .take_while(|&ch| ch.is_alphanumeric() || matches!(ch, '/' | '-' | '_' | '.'))
            .collect();
        let end = path.find(".md")?;
        Some(path[..end + 3].to_owned())
    };
    // `, "Heading"` right after a path, a closing backtick allowed between.
    let heading_after = |rest: &str| {
        rest.trim_start_matches('`')
            .strip_prefix(", \"")
            .and_then(|quoted| quoted.find('"').map(|end| quoted[..end].to_owned()))
    };
    let mut cited = Vec::new();
    for (at, _) in comment.match_indices("docs/src/") {
        if let Some(path) = path_at(&comment[at..]) {
            let heading = heading_after(&comment[at + path.len()..]);
            cited.push((path, heading, at));
        }
    }
    for (at, marker) in comment.match_indices("See the book:") {
        let rest = comment[at + marker.len()..].trim_start();
        let Some(path) = path_at(rest) else {
            continue;
        };
        let heading = heading_after(&rest[path.len()..]);
        cited.push((format!("docs/src/{path}"), heading, at));
    }
    cited
}

/// Returns whether `page` holds `heading` as a heading line, backticks aside.
///
/// A heading line is one to six `#`, a space, then the heading. Backticks are
/// left out on both sides, so a comment may quote an identifier in code.
pub fn has_heading(page: &str, heading: &str) -> bool {
    let bare = |text: &str| text.replace('`', "");
    let cited = bare(heading);
    page.lines().any(|line| {
        let hashes = line.chars().take_while(|&ch| ch == '#').count();
        (1..=6).contains(&hashes)
            && line[hashes..]
                .strip_prefix(' ')
                .is_some_and(|text| bare(text.trim_end()) == cited)
    })
}

/// Names each comment line of `text` that carries a plan or review label,
/// and each inline `//` comment opening on a numbered step.
pub fn plan_labels(text: &str) -> Vec<(usize, String)> {
    let comments = comments_of(text);
    let mut found = Vec::new();
    for run in runs(&comments) {
        // A run is read whole, so a cited heading that wraps is blanked whole.
        let mut joined = String::new();
        let mut spans = Vec::with_capacity(run.len());
        for comment in run {
            let start = joined.len();
            joined.push_str(&comment.text);
            spans.push(start..joined.len());
            joined.push(' ');
        }
        let blanked = without_cited_headings(&joined);
        for (comment, span) in run.iter().zip(spans) {
            let numbered =
                comment.kind == CommentKind::Line && opens_on_numbered_step(&comment.text);
            if numbered || names_plan_label(&blanked[span]) {
                found.push((comment.line, comment.text.trim().to_owned()));
            }
        }
    }
    found
}

/// Names each `///` block of `text` whose first paragraph runs over two
/// lines, at the paragraph's first line.
pub fn long_doc_summaries(text: &str) -> Vec<(usize, String)> {
    let docs: Vec<Comment> = comments_of(text)
        .into_iter()
        .filter(|comment| comment.kind == CommentKind::Doc && comment.whole_line)
        .collect();
    let mut found = Vec::new();
    for block in runs(&docs) {
        let blank = |comment: &&Comment| comment.text[3..].trim().is_empty();
        let mut lines = block.iter().skip_while(blank).peekable();
        let Some(first) = lines.peek().map(|comment| comment.line) else {
            continue;
        };
        let summary = lines.take_while(|comment| !blank(comment)).count();
        if summary > 2 {
            found.push((first, format!("summary of {summary} lines")));
        }
    }
    found
}

/// Names each run of more than four indented `//` lines in `text`; a run
/// opening on `SAFETY:` passes.
pub fn long_inline_comments(text: &str) -> Vec<(usize, String)> {
    let inline: Vec<Comment> = comments_of(text)
        .into_iter()
        .filter(|comment| {
            comment.kind == CommentKind::Line && comment.whole_line && comment.indented
        })
        .collect();
    runs(&inline)
        .into_iter()
        .filter(|run| {
            let opening = run[0].text.trim_start_matches('/').trim_start();
            run.len() > 4 && !opening.starts_with("SAFETY:")
        })
        .map(|run| (run[0].line, format!("{} comment lines", run.len())))
        .collect()
}

/// Names each book citation in the comments of `text` whose page, or whose
/// heading in that page, is missing under `root`.
pub fn missing_book_citations(root: &Path, text: &str) -> Vec<(usize, String)> {
    let comments = comments_of(text);
    let mut found = Vec::new();
    for (at, comment) in comments.iter().enumerate() {
        // A citation wrapped onto the next comment line is read whole.
        let next = comments
            .get(at + 1)
            .filter(|next| next.line == comment.line + 1)
            .map_or("", |next| {
                next.text.trim_start_matches(['/', '!', '*']).trim_start()
            });
        let joined = format!("{} {next}", comment.text.trim_end());
        let own = comment.text.trim_end().len();
        for (page, heading) in book_citations(&joined)
            .into_iter()
            .filter(|(_, _, start)| *start < own)
            .map(|(page, heading, _)| (page, heading))
        {
            let file = root.join(&page);
            let missing = if !file.is_file() {
                Some(format!("no page `{page}`"))
            } else {
                heading
                    .filter(|heading| !has_heading(&read_text(&file), heading))
                    .map(|heading| format!("no heading \"{heading}\" in `{page}`"))
            };
            if let Some(missing) = missing {
                found.push((comment.line, missing));
            }
        }
    }
    found
}

/// Collects every comment line under `root` naming a plan or review label,
/// or opening on a numbered step, as `path:line: comment`.
///
/// Reads every `.rs` file of `crates/*/src`, `crates/*/tests`, `xtask/src`,
/// `xtask/tests` and `examples/*/src`; block comments count.
pub fn plan_label_problems(root: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    for (path, text) in comment_scanned_files(root) {
        for (line, comment) in plan_labels(&text) {
            problems.push(format!("{path}:{line}: {comment}"));
        }
    }
    problems
}

/// Collects, per non-test file of `root`'s `src` trees, each `///` block
/// whose first paragraph runs over two lines, as `line N: summary of K lines`.
pub fn doc_summary_violations(root: &Path) -> Violations {
    per_source_violations(root, long_doc_summaries)
}

/// Collects, per non-test file of `root`'s `src` trees, each run of more
/// than four indented `//` lines, as `line N: K comment lines`.
pub fn inline_comment_violations(root: &Path) -> Violations {
    per_source_violations(root, long_inline_comments)
}

/// Collects every book citation under `root` naming a missing page or
/// heading, as `path:line: what is missing`, in the files of [`plan_label_problems`].
pub fn book_citation_problems(root: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    for (path, text) in comment_scanned_files(root) {
        for (line, missing) in missing_book_citations(root, &text) {
            problems.push(format!("{path}:{line}: {missing}"));
        }
    }
    problems
}

/// Runs `check` on each non-test file of `root`'s `src` trees, keyed by its
/// path, each finding as `line N: …`.
pub fn per_source_violations(root: &Path, check: fn(&str) -> Vec<(usize, String)>) -> Violations {
    let mut violations = Violations::new();
    for src in rust_src_roots(root) {
        for (path, text) in non_test_rust_files(&src) {
            for (line, message) in check(&text) {
                add(
                    &mut violations,
                    rel(root, &path),
                    format!("line {line}: {message}"),
                );
            }
        }
    }
    violations
}

#[cfg(test)]
mod tests {
    use super::has_heading;

    #[test]
    fn a_heading_is_found_with_or_without_backticks_on_either_side() {
        let page = "# Title\n\n## AdaptationMode — the surface\n\n### `Lane` and `Layer`\n";
        assert!(has_heading(page, "AdaptationMode — the surface"));
        assert!(has_heading(page, "`AdaptationMode` — the surface"));
        assert!(has_heading(page, "Lane and Layer"));
        assert!(has_heading(page, "`Lane` and `Layer`"));
        assert!(!has_heading(page, "AdaptationMode"));
    }
}

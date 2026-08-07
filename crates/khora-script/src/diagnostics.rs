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

//! Source positions and error reporting.
//!
//! Ergon's whole argument for existing is that it fits gameplay work better
//! than a general-purpose language. A compiler whose errors read like
//! `unexpected token at 1247` fails that argument on first contact, so
//! diagnostics are built first and everything downstream is required to use
//! them.
//!
//! Every error carries a [`Span`], and a [`Diagnostic`] renders as the offending
//! line with a caret under the exact characters at fault:
//!
//! ```text
//! error: cannot mix a duration with an angle
//!   --> patrol.erg:12:19
//!    |
//! 12 |     var wrong = 2s + 90deg;
//!    |                 ^^^^^^^^^^
//!    = note: `2s` is a Duration, `90deg` is an Angle
//! ```
//!
//! Byte offsets are stored rather than line/column pairs: the lexer advances
//! through bytes, so offsets are what it naturally has, and line/column is
//! computed on demand — only when something is actually being reported, which
//! is the rare path.

use std::fmt;

/// A half-open byte range `[start, end)` into a [`SourceFile`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Span {
    /// Byte offset of the first character.
    pub start: u32,
    /// Byte offset one past the last character.
    pub end: u32,
}

impl Span {
    /// A span covering `[start, end)`.
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    /// An empty span at `offset`, for errors that point between characters —
    /// an unexpected end of file, say.
    pub const fn empty(offset: u32) -> Self {
        Self {
            start: offset,
            end: offset,
        }
    }

    /// The smallest span covering both. Used to report a whole expression when
    /// only its ends are known.
    pub fn to(self, other: Self) -> Self {
        Self {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }

    /// Length in bytes.
    pub const fn len(self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    /// Whether the span covers no characters.
    pub const fn is_empty(self) -> bool {
        self.end <= self.start
    }
}

/// One source file, with the line index needed to turn offsets into positions.
#[derive(Debug, Clone)]
pub struct SourceFile {
    name: String,
    text: String,
    /// Byte offset of the first character of each line. Built once at load, so
    /// resolving a position is a binary search rather than a rescan.
    line_starts: Vec<u32>,
}

impl SourceFile {
    /// Indexes `text`, remembering `name` for error messages.
    pub fn new(name: impl Into<String>, text: impl Into<String>) -> Self {
        let text = text.into();
        let mut line_starts = vec![0];
        for (offset, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                // `offset + 1` is the start of the next line; if the file ends
                // with a newline this records an empty final line, which is
                // correct — an error there should not be attributed to the
                // previous line.
                line_starts.push(offset as u32 + 1);
            }
        }
        Self {
            name: name.into(),
            text,
            line_starts,
        }
    }

    /// The file's name, as it appears in diagnostics.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The full source text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The 1-based line and column of `offset`.
    ///
    /// Column counts *characters*, not bytes, so a line with accents does not
    /// report a column past what the reader can count.
    pub fn line_col(&self, offset: u32) -> (u32, u32) {
        let line_index = match self.line_starts.binary_search(&offset) {
            Ok(exact) => exact,
            // `Err(i)` means the offset sorts before `line_starts[i]`, so it
            // belongs to the line before it.
            Err(next) => next.saturating_sub(1),
        };
        let line_start = self.line_starts.get(line_index).copied().unwrap_or(0);
        let column = self
            .text
            .get(line_start as usize..offset as usize)
            .map(|prefix| prefix.chars().count() as u32)
            .unwrap_or(0);
        (line_index as u32 + 1, column + 1)
    }

    /// The text of the 1-based line `line`, without its terminator.
    pub fn line_text(&self, line: u32) -> &str {
        let index = line.saturating_sub(1) as usize;
        // A line that does not exist yields nothing. Falling back to offset 0
        // here would return the whole file, which is how a caret ends up under
        // an unrelated line in an error message.
        let Some(start) = self.line_starts.get(index).map(|offset| *offset as usize) else {
            return "";
        };
        let end = self
            .line_starts
            .get(index + 1)
            .map(|next| *next as usize)
            .unwrap_or(self.text.len());
        self.text
            .get(start..end)
            .unwrap_or("")
            .trim_end_matches(['\n', '\r'])
    }
}

/// How much a diagnostic matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Compilation cannot produce a module.
    Error,
    /// Compilation continues, but something looks wrong.
    Warning,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Error => write!(f, "error"),
            Self::Warning => write!(f, "warning"),
        }
    }
}

/// A single problem with the source.
///
/// The `note` is where the *why* goes. An error that only states the rule
/// leaves the reader to guess the intent behind it, which is how a language
/// earns a reputation for being obtuse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Error or warning.
    pub severity: Severity,
    /// One line, lower case, no trailing period — the convention rustc uses.
    pub message: String,
    /// What to underline.
    pub span: Span,
    /// Optional second line explaining the reasoning or the fix.
    pub note: Option<String>,
}

/// Whether any of these stops the pipeline.
///
/// One definition, because six places asked it: the five compiler stages each
/// spelled `iter().any(|d| d.severity == Severity::Error)` on their own result
/// type, and `khora-io`'s driver spelled the element predicate a sixth time. A
/// stage result is genuinely its own type; the question asked of it is not.
pub fn has_errors(diagnostics: &[Diagnostic]) -> bool {
    diagnostics.iter().any(Diagnostic::is_error)
}

impl Diagnostic {
    /// Whether this one stops the pipeline.
    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }

    /// An error at `span`.
    pub fn error(message: impl Into<String>, span: Span) -> Self {
        Self {
            severity: Severity::Error,
            message: message.into(),
            span,
            note: None,
        }
    }

    /// A warning at `span`.
    pub fn warning(message: impl Into<String>, span: Span) -> Self {
        Self {
            severity: Severity::Warning,
            message: message.into(),
            span,
            note: None,
        }
    }

    /// Attaches the explanation.
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    /// Renders against `source`, with the offending line and a caret run.
    pub fn render(&self, source: &SourceFile) -> String {
        let (line, column) = source.line_col(self.span.start);
        let gutter_width = line.to_string().len();
        let pad = " ".repeat(gutter_width);
        let line_text = source.line_text(line);

        // The caret run is measured in characters so it lines up under the
        // source on any line, and clamped to the line so a multi-line span does
        // not draw a run of carets off the right edge.
        let before = line_text.chars().take(column.saturating_sub(1) as usize);
        let indent: String = before.map(|c| if c == '\t' { '\t' } else { ' ' }).collect();
        let visible = line_text.chars().count() as u32;
        let caret_len = self
            .span
            .len()
            .max(1)
            .min(visible.saturating_sub(column - 1).max(1));

        let mut out = format!(
            "{}: {}\n{}--> {}:{}:{}\n{} |\n{} | {}\n{} | {}{}",
            self.severity,
            self.message,
            pad,
            source.name(),
            line,
            column,
            pad,
            line,
            line_text,
            pad,
            indent,
            "^".repeat(caret_len as usize),
        );

        if let Some(note) = &self.note {
            out.push_str(&format!("\n{pad} = note: {note}"));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> SourceFile {
        SourceFile::new("patrol.erg", "behavior Guard {\n    float speed = 3;\n}\n")
    }

    #[test]
    fn offsets_resolve_to_line_and_column() {
        let source = sample();
        assert_eq!(source.line_col(0), (1, 1));
        assert_eq!(source.line_col(9), (1, 10), "the G of Guard");
        assert_eq!(source.line_col(17), (2, 1), "start of the second line");
        assert_eq!(source.line_col(21), (2, 5), "the f of float");
    }

    /// Column counts characters, not bytes — otherwise any accented line
    /// reports a column the reader cannot count to.
    #[test]
    fn columns_count_characters_not_bytes() {
        // `é` is two bytes, so "// héllo" spans offsets 0..9 — the newline sits
        // at 9 as the line's 9th character, and the next line starts at 10.
        let source = SourceFile::new("x.erg", "// héllo\nvar x = 1;");
        assert_eq!(source.line_col(9), (1, 9), "the newline ends line 1");
        assert_eq!(source.line_col(10), (2, 1), "`var` opens line 2");

        let accented = SourceFile::new("y.erg", "var é = 1;");
        // `é` is two bytes, so the byte offset of `=` is 7 but it is the 7th
        // character.
        assert_eq!(accented.line_col(7), (1, 7));
    }

    #[test]
    fn line_text_excludes_the_terminator() {
        let source = sample();
        assert_eq!(source.line_text(1), "behavior Guard {");
        assert_eq!(source.line_text(2), "    float speed = 3;");
        assert_eq!(source.line_text(3), "}");
    }

    #[test]
    fn a_line_past_the_end_is_empty_rather_than_panicking() {
        assert_eq!(sample().line_text(99), "");
    }

    #[test]
    fn spans_merge_to_cover_both() {
        let merged = Span::new(4, 8).to(Span::new(12, 20));
        assert_eq!(merged, Span::new(4, 20));
        assert_eq!(Span::new(12, 20).to(Span::new(4, 8)), merged, "order-free");
    }

    #[test]
    fn rendering_points_at_the_offending_text() {
        let source = sample();
        // "speed" on line 2.
        let rendered = Diagnostic::error("unknown field `speed`", Span::new(27, 32))
            .with_note("declare it in the behavior body")
            .render(&source);

        assert!(rendered.contains("error: unknown field `speed`"));
        assert!(rendered.contains("patrol.erg:2:11"));
        assert!(rendered.contains("    float speed = 3;"));
        assert!(rendered.contains("^^^^^"), "five carets under `speed`");
        assert!(rendered.contains("= note: declare it in the behavior body"));
    }

    /// A caret run must never spill past the end of the line: a span covering
    /// several lines is reported on the first, not drawn into empty space.
    #[test]
    fn carets_stay_within_the_line() {
        let source = sample();
        let rendered = Diagnostic::error("spans the file", Span::new(0, 40)).render(&source);

        let caret_line = rendered
            .lines()
            .last()
            .expect("the render ends with the caret line");
        let carets = caret_line.chars().filter(|c| *c == '^').count();
        assert!(carets <= source.line_text(1).chars().count());
        assert!(carets > 0, "something must be underlined");
    }

    /// An empty span still underlines one character, so an "unexpected end of
    /// input" error is visible rather than silently caret-less.
    #[test]
    fn an_empty_span_still_underlines_something() {
        let source = sample();
        let rendered =
            Diagnostic::error("unexpected end of input", Span::empty(16)).render(&source);
        assert!(rendered.contains('^'));
    }
}

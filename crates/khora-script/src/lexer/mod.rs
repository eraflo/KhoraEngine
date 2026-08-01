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

//! Source text to tokens.
//!
//! Two things here are not what a C#-shaped lexer would do, and both are
//! deliberate.
//!
//! **Units are lexed, not parsed.** `2s`, `500ms` and `90deg` are single
//! literals, so `Duration` and `Angle` are real types from the very first pass
//! instead of floats that a later stage tries to tell apart. Mixing radians and
//! degrees then stops being a class of bug and becomes a compile error.
//!
//! **A float needs no `f`.** In gameplay code the overwhelming majority of
//! numbers are floats, so `2.0` is one and the suffix noise disappears. Whole
//! numbers still lex as `int`; the type checker widens them where a float is
//! expected, which is the one implicit conversion the language allows.
//!
//! The lexer never fails on the first bad character. It records a diagnostic,
//! skips, and keeps going — one run should report every lexical problem in the
//! file, not send the author back for another round after each one.

pub mod scanner;
pub mod token;

pub use token::{Keyword, Token, TokenKind};

use self::scanner::Lexer;
use crate::diagnostics::Diagnostic;

/// What a run of lexing produced.
///
/// Tokens *and* diagnostics: a file with errors still yields the tokens it
/// managed to read, so later stages can report more than the first problem.
#[derive(Debug, Clone)]
pub struct Lexed {
    /// The tokens, always ending with [`TokenKind::Eof`].
    pub tokens: Vec<Token>,
    /// Problems found. Empty means the file lexed cleanly.
    pub diagnostics: Vec<Diagnostic>,
}

impl Lexed {
    /// Whether any diagnostic is an error.
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == crate::diagnostics::Severity::Error)
    }
}

/// Turns `source` into tokens.
pub fn lex(source: &str) -> Lexed {
    Lexer::new(source).run()
}

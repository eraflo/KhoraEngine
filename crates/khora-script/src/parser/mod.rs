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

//! Tokens to syntax tree.
//!
//! A hand-written recursive-descent parser. A generator would save some of this
//! code, but the error messages are the product here — the case for a bespoke
//! language collapses if its compiler explains itself worse than the one it
//! replaces — and generated parsers are hard to make apologise well.
//!
//! # Recovering from errors
//!
//! On a syntax error the parser records it, then skips to the next statement or
//! declaration boundary and carries on. One run reports several problems
//! instead of one per compile-run-fix cycle. The cost is that a badly broken
//! file can produce follow-on noise; the cure is to stop at a boundary the
//! author would recognise — a `;`, a `}`, or a keyword that can only start a
//! declaration.
//!
//! # What is *not* checked here
//!
//! Whether `await` sits inside an `async` member, whether a `T?` was tested
//! before use, whether units match. All of it is well-formed syntax and belongs
//! to the type checker, which can name the enclosing member and the types
//! involved. Rejecting them here would produce a message about grammar for a
//! problem about meaning.

use self::describe::describe;
use crate::ast::*;
use crate::diagnostics::{Diagnostic, Span};
use crate::lexer::{Keyword, Token, TokenKind};

/// A parsed module, plus whatever went wrong.
#[derive(Debug, Clone)]
pub struct Parsed {
    /// The tree. Present even when errors occurred — partial, but useful.
    pub module: Module,
    /// Problems found.
    pub diagnostics: Vec<Diagnostic>,
}

impl Parsed {
    /// Whether any diagnostic is an error.
    pub fn has_errors(&self) -> bool {
        crate::diagnostics::has_errors(&self.diagnostics)
    }
}

/// Parses `tokens` into a module.
pub fn parse(tokens: Vec<Token>) -> Parsed {
    Parser::new(tokens).run()
}

/// Signals that the parser gave up on the current construct and should recover.
/// The diagnostic is already recorded when this travels.
pub(super) struct Bail;

pub(super) type Parse<T> = Result<T, Bail>;

struct Parser {
    tokens: Vec<Token>,
    position: usize,
    diagnostics: Vec<Diagnostic>,
}

impl Parser {
    pub(super) fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            position: 0,
            diagnostics: Vec::new(),
        }
    }

    pub(super) fn run(mut self) -> Parsed {
        let mut imports = Vec::new();
        let mut items = Vec::new();

        while !self.at_end() {
            if self.check_keyword(Keyword::Import) {
                match self.parse_import() {
                    Ok(import) => imports.push(import),
                    Err(Bail) => self.recover_to_item(),
                }
                continue;
            }

            match self.parse_item() {
                Ok(item) => items.push(item),
                Err(Bail) => self.recover_to_item(),
            }
        }

        Parsed {
            module: Module { imports, items },
            diagnostics: self.diagnostics,
        }
    }

    // ── Cursor ────────────────────────────────────────

    pub(super) fn peek(&self) -> &TokenKind {
        self.tokens
            .get(self.position)
            .map(|t| &t.kind)
            .unwrap_or(&TokenKind::Eof)
    }

    pub(super) fn peek_at(&self, ahead: usize) -> &TokenKind {
        self.tokens
            .get(self.position + ahead)
            .map(|t| &t.kind)
            .unwrap_or(&TokenKind::Eof)
    }

    pub(super) fn span(&self) -> Span {
        self.tokens
            .get(self.position)
            .map(|t| t.span)
            .unwrap_or_else(|| self.tokens.last().map(|t| t.span).unwrap_or(Span::empty(0)))
    }

    pub(super) fn previous_span(&self) -> Span {
        self.tokens
            .get(self.position.saturating_sub(1))
            .map(|t| t.span)
            .unwrap_or(Span::empty(0))
    }

    pub(super) fn at_end(&self) -> bool {
        matches!(self.peek(), TokenKind::Eof)
    }

    pub(super) fn advance(&mut self) -> TokenKind {
        let kind = self.peek().clone();
        if !self.at_end() {
            self.position += 1;
        }
        kind
    }

    pub(super) fn check(&self, kind: &TokenKind) -> bool {
        self.peek() == kind
    }

    pub(super) fn check_keyword(&self, keyword: Keyword) -> bool {
        matches!(self.peek(), TokenKind::Keyword(k) if *k == keyword)
    }

    pub(super) fn eat(&mut self, kind: &TokenKind) -> bool {
        if self.check(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    pub(super) fn eat_keyword(&mut self, keyword: Keyword) -> bool {
        if self.check_keyword(keyword) {
            self.advance();
            true
        } else {
            false
        }
    }

    pub(super) fn expect(&mut self, kind: TokenKind, what: &str) -> Parse<Span> {
        if self.check(&kind) {
            let span = self.span();
            self.advance();
            return Ok(span);
        }
        Err(self.error_here(format!("expected {what}")))
    }

    pub(super) fn expect_ident(&mut self, what: &str) -> Parse<(String, Span)> {
        if let TokenKind::Ident(name) = self.peek().clone() {
            let span = self.span();
            self.advance();
            return Ok((name, span));
        }
        Err(self.error_here(format!("expected {what}")))
    }

    /// Records an error at the current token and signals a bail.
    pub(super) fn error_here(&mut self, message: impl Into<String>) -> Bail {
        let found = describe(self.peek());
        let span = self.span();
        self.diagnostics.push(Diagnostic::error(
            format!("{}, found {found}", message.into()),
            span,
        ));
        Bail
    }

    pub(super) fn error_with_note(
        &mut self,
        message: impl Into<String>,
        note: impl Into<String>,
    ) -> Bail {
        let found = describe(self.peek());
        let span = self.span();
        self.diagnostics.push(
            Diagnostic::error(format!("{}, found {found}", message.into()), span).with_note(note),
        );
        Bail
    }

    // ── Recovery ──────────────────────────────────────

    /// Skips to something that can begin a top-level declaration.
    pub(super) fn recover_to_item(&mut self) {
        // Always consume at least one token, or a token that cannot start a
        // declaration would spin forever.
        if !self.at_end() {
            self.advance();
        }
        while !self.at_end() {
            if matches!(
                self.peek(),
                TokenKind::Keyword(
                    Keyword::Behavior | Keyword::Struct | Keyword::Fn | Keyword::Import
                )
            ) {
                return;
            }
            self.advance();
        }
    }

    /// Skips to the end of the current statement.
    pub(super) fn recover_to_statement(&mut self) {
        while !self.at_end() {
            if self.eat(&TokenKind::Semi) {
                return;
            }
            // A closing brace ends the enclosing block: consuming it would
            // desynchronise the nesting for everything that follows.
            if self.check(&TokenKind::RBrace) {
                return;
            }
            self.advance();
        }
    }

    // ── Sub-parsers ───────────────────────────────────
    //
    // Split by what is being parsed. Each file adds methods to `Parser`, so the
    // cursor and recovery stay in one place while the grammar for declarations,
    // types, statements and expressions each gets its own file.
}

mod decl;
mod describe;
mod expr;
mod stmt;
mod types;

#[cfg(test)]
mod tests;

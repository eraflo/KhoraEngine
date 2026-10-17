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

//! `$"hp: {h.current}"` parsed: its text, and each hole's one expression.
//!
//! A hole is parsed on its own, from the tokens the lexer read for it. A
//! ternary in one is written in parentheses: the `:` is kept for what a hole
//! may one day say about how it is shown.

use super::{Parse, Parser};
use crate::ast::{Expr, InterpPiece};
use crate::diagnostics::{Diagnostic, Span};
use crate::lexer::{Piece, Token, TokenKind};

impl Parser {
    /// The interpolated string at the cursor, `pieces` its lexed parts.
    pub(super) fn parse_interpolated(&mut self, pieces: Vec<Piece>, span: Span) -> Parse<Expr> {
        self.advance();
        let mut parsed = Vec::with_capacity(pieces.len());
        for piece in pieces {
            match piece {
                Piece::Text(text) => parsed.push(InterpPiece::Text(text)),
                Piece::Hole { tokens, .. } => {
                    if let Some(hole) = self.parse_hole(tokens) {
                        parsed.push(InterpPiece::Hole(hole));
                    }
                }
            }
        }
        // Text alone is a string literal, compiled as one.
        if parsed
            .iter()
            .all(|piece| matches!(piece, InterpPiece::Text(_)))
        {
            let value = parsed
                .into_iter()
                .map(|piece| match piece {
                    InterpPiece::Text(text) => text,
                    InterpPiece::Hole(_) => String::new(),
                })
                .collect();
            return Ok(Expr::Str { value, span });
        }
        Ok(Expr::Interpolated {
            pieces: parsed,
            span,
        })
    }

    /// One hole's expression, its problems reported where they sit.
    fn parse_hole(&mut self, tokens: Vec<Token>) -> Option<Expr> {
        if let Some(ternary) = bare_ternary(&tokens) {
            self.diagnostics.push(
                Diagnostic::error(
                    "a ternary in a hole is written in parentheses: write `(a ? b : c)` inside `{}`",
                    ternary,
                )
                .with_note("the `:` is kept for what a hole may one day say about how it is shown"),
            );
            return None;
        }
        let mut inner = Parser::new(tokens);
        let parsed = inner.parse_expr();
        if parsed.is_ok() && !inner.at_end() {
            let at = inner.span();
            inner.diagnostics.push(
                Diagnostic::error("a hole holds one expression", at)
                    .with_note("close it with `}` after the value"),
            );
        }
        let failed = parsed.is_err() || !inner.diagnostics.is_empty();
        self.diagnostics.append(&mut inner.diagnostics);
        match parsed {
            Ok(expr) if !failed => Some(expr),
            _ => None,
        }
    }
}

/// Where a ternary written bare in a hole sits — its `?` outside any
/// parentheses, brackets or braces.
fn bare_ternary(tokens: &[Token]) -> Option<Span> {
    let mut depth = 0usize;
    let mut question = false;
    for token in tokens {
        match token.kind {
            TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace => depth += 1,
            TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => {
                depth = depth.saturating_sub(1);
            }
            TokenKind::Question if depth == 0 => question = true,
            _ => {}
        }
    }
    let written: Vec<&Token> = tokens
        .iter()
        .filter(|token| token.kind != TokenKind::Eof)
        .collect();
    match (question, written.first(), written.last()) {
        (true, Some(first), Some(last)) => Some(first.span.to(last.span)),
        _ => None,
    }
}

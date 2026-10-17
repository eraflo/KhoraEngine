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

//! The arguments between a call's parentheses: positional ones first, then
//! named ones — `Move(1, speed: 2.0, turns: 3)`.
//!
//! `name :` opening an argument is a name, and nothing else can open one so:
//! a ternary's `:` follows a `?` inside its argument, a struct literal opens
//! with `Name {`.

use super::{Parse, Parser};
use crate::ast::{Expr, Label};
use crate::diagnostics::Diagnostic;
use crate::lexer::TokenKind;

impl Parser {
    /// The arguments up to the closing `)`, which is left for the caller —
    /// with the name each is given, empty when none is named.
    pub(super) fn parse_arguments(&mut self) -> Parse<(Vec<Expr>, Vec<Option<Label>>)> {
        let mut args = Vec::new();
        let mut labels: Vec<Option<Label>> = Vec::new();
        if self.check(&TokenKind::RParen) {
            return Ok((args, labels));
        }
        loop {
            let label = match (self.peek().clone(), self.peek_at(1)) {
                (TokenKind::Ident(name), TokenKind::Colon) => {
                    let span = self.span();
                    self.advance();
                    self.advance();
                    Some(Label { name, span })
                }
                _ => None,
            };
            let value = self.parse_expr()?;
            if label.is_none() {
                if let Some(first) = labels.iter().flatten().next() {
                    self.diagnostics.push(
                        Diagnostic::error(
                            "a positional argument cannot follow a named one",
                            value.span(),
                        )
                        .with_note(format!("name it, or move it before `{}:`", first.name)),
                    );
                }
            }
            args.push(value);
            labels.push(label);
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        if labels.iter().all(Option::is_none) {
            labels.clear();
        }
        Ok((args, labels))
    }

    /// Refuses the names given to arguments where a call takes none, saying
    /// `why` at the first.
    pub(super) fn refuse_labels(&mut self, labels: &[Option<Label>], why: &str) {
        if let Some(first) = labels.iter().flatten().next() {
            self.diagnostics
                .push(Diagnostic::error(why.to_owned(), first.span));
        }
    }
}

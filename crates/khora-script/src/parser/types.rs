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

//! Parsing written types, including the `[]` and `?` suffixes that stack.

use super::{Parse, Parser};
use crate::ast::TypeRef;
use crate::lexer::{Keyword, TokenKind};

impl Parser {
    // ── Types ─────────────────────────────────────────

    pub(super) fn parse_type(&mut self) -> Parse<TypeRef> {
        if self.eat_keyword(Keyword::Void) {
            return Ok(TypeRef::Void);
        }

        let start = self.span();
        let (name, name_span) = self.expect_ident("a type")?;

        let mut ty = if self.check(&TokenKind::Less) {
            self.advance();
            let mut args = Vec::new();
            loop {
                args.push(self.parse_type()?);
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
            let end = self.expect(TokenKind::Greater, "`>` to close the type arguments")?;
            TypeRef::Generic {
                name,
                args,
                span: start.to(end),
            }
        } else {
            TypeRef::Named {
                name,
                span: name_span,
            }
        };

        // Suffixes stack: `T[]?` is an optional array, `T?[]` an array of
        // optionals. Left-to-right application keeps that readable.
        loop {
            if self.check(&TokenKind::LBracket) && matches!(self.peek_at(1), TokenKind::RBracket) {
                self.advance();
                let end = self.span();
                self.advance();
                ty = TypeRef::Array {
                    element: Box::new(ty),
                    span: start.to(end),
                };
                continue;
            }
            if self.check(&TokenKind::Question) {
                let end = self.span();
                self.advance();
                ty = TypeRef::Optional {
                    inner: Box::new(ty),
                    span: start.to(end),
                };
                continue;
            }
            break;
        }

        Ok(ty)
    }
}

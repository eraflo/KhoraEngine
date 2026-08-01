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

//! Parsing statements.

use super::{Bail, Parse, Parser};
use crate::ast::*;
use crate::lexer::{Keyword, TokenKind};

impl Parser {
    // ── Statements ────────────────────────────────────

    pub(super) fn parse_block(&mut self) -> Parse<Block> {
        let start = self.expect(TokenKind::LBrace, "`{` to open a block")?;
        let mut statements = Vec::new();

        while !self.check(&TokenKind::RBrace) && !self.at_end() {
            match self.parse_statement() {
                Ok(statement) => statements.push(statement),
                Err(Bail) => self.recover_to_statement(),
            }
        }

        let end = self.expect(TokenKind::RBrace, "`}` to close the block")?;
        Ok(Block {
            statements,
            span: start.to(end),
        })
    }

    pub(super) fn parse_statement(&mut self) -> Parse<Stmt> {
        match self.peek().clone() {
            TokenKind::Keyword(Keyword::If) => self.parse_if(),
            TokenKind::Keyword(Keyword::While) => self.parse_while(),
            TokenKind::Keyword(Keyword::For) => self.parse_for(),
            TokenKind::Keyword(Keyword::Foreach) => self.parse_foreach(),
            TokenKind::Keyword(Keyword::Return) => self.parse_return(),
            TokenKind::Keyword(Keyword::Become) => self.parse_become(),
            TokenKind::Keyword(Keyword::Match) => self.parse_match(),
            TokenKind::Keyword(Keyword::Break) => {
                let span = self.span();
                self.advance();
                let end = self.expect(TokenKind::Semi, "`;` after `break`")?;
                Ok(Stmt::Break(span.to(end)))
            }
            TokenKind::Keyword(Keyword::Continue) => {
                let span = self.span();
                self.advance();
                let end = self.expect(TokenKind::Semi, "`;` after `continue`")?;
                Ok(Stmt::Continue(span.to(end)))
            }
            TokenKind::Keyword(Keyword::Var) => self.parse_var(),
            TokenKind::LBrace => Ok(Stmt::Block(self.parse_block()?)),
            _ => {
                if self.looks_like_typed_declaration() {
                    return self.parse_typed_let();
                }
                let expr = self.parse_expr()?;
                let end = self.expect(TokenKind::Semi, "`;` after the expression")?;
                let span = expr.span().to(end);
                let _ = span;
                Ok(Stmt::Expr(expr))
            }
        }
    }

    /// Whether the statement is `Type name …` rather than an expression.
    ///
    /// `Vec3 target = …;` and `target = …;` both start with an identifier, so
    /// the decision is made by looking for a second name before the `=`.
    pub(super) fn looks_like_typed_declaration(&self) -> bool {
        if !matches!(self.peek(), TokenKind::Ident(_)) {
            return false;
        }
        let mut ahead = 1;
        loop {
            match self.peek_at(ahead) {
                TokenKind::LBracket if matches!(self.peek_at(ahead + 1), TokenKind::RBracket) => {
                    ahead += 2;
                }
                TokenKind::Question => ahead += 1,
                _ => break,
            }
        }
        matches!(self.peek_at(ahead), TokenKind::Ident(_))
            && matches!(self.peek_at(ahead + 1), TokenKind::Assign | TokenKind::Semi)
    }

    pub(super) fn parse_typed_let(&mut self) -> Parse<Stmt> {
        let start = self.span();
        let ty = self.parse_type()?;
        let (name, _) = self.expect_ident("a variable name")?;
        let value = if self.eat(&TokenKind::Assign) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        let end = self.expect(TokenKind::Semi, "`;` after the declaration")?;
        Ok(Stmt::Let {
            ty: Some(ty),
            name,
            value,
            span: start.to(end),
        })
    }

    pub(super) fn parse_var(&mut self) -> Parse<Stmt> {
        let start = self.span();
        self.advance(); // `var`
        let (name, _) = self.expect_ident("a variable name after `var`")?;

        if !self.check(&TokenKind::Assign) {
            return Err(self.error_with_note(
                "expected `=` after a `var` declaration",
                "`var` infers the type from the initialiser, so it needs one — write the type instead if there is nothing to infer from",
            ));
        }
        self.advance();

        let value = self.parse_expr()?;
        let end = self.expect(TokenKind::Semi, "`;` after the declaration")?;
        Ok(Stmt::Let {
            ty: None,
            name,
            value: Some(value),
            span: start.to(end),
        })
    }

    pub(super) fn parse_if(&mut self) -> Parse<Stmt> {
        let start = self.span();
        self.advance(); // `if`
        self.expect(TokenKind::LParen, "`(` after `if`")?;
        let condition = self.parse_expr()?;
        self.expect(TokenKind::RParen, "`)` after the condition")?;

        let then_branch = self.parse_block_or_statement()?;
        let mut span = start.to(then_branch.span);

        let else_branch = if self.eat_keyword(Keyword::Else) {
            // `else if` chains without needing a block, as in C#.
            let branch = if self.check_keyword(Keyword::If) {
                self.parse_if()?
            } else {
                Stmt::Block(self.parse_block_or_statement()?)
            };
            span = span.to(branch.span());
            Some(Box::new(branch))
        } else {
            None
        };

        Ok(Stmt::If {
            condition,
            then_branch,
            else_branch,
            span,
        })
    }

    /// A braced block, or a single statement treated as one.
    pub(super) fn parse_block_or_statement(&mut self) -> Parse<Block> {
        if self.check(&TokenKind::LBrace) {
            return self.parse_block();
        }
        let statement = self.parse_statement()?;
        let span = statement.span();
        Ok(Block {
            statements: vec![statement],
            span,
        })
    }

    pub(super) fn parse_while(&mut self) -> Parse<Stmt> {
        let start = self.span();
        self.advance();
        self.expect(TokenKind::LParen, "`(` after `while`")?;
        let condition = self.parse_expr()?;
        self.expect(TokenKind::RParen, "`)` after the condition")?;
        let body = self.parse_block_or_statement()?;
        let span = start.to(body.span);
        Ok(Stmt::While {
            condition,
            body,
            span,
        })
    }

    pub(super) fn parse_for(&mut self) -> Parse<Stmt> {
        let start = self.span();
        self.advance();
        self.expect(TokenKind::LParen, "`(` after `for`")?;

        let init = if self.check(&TokenKind::Semi) {
            self.advance();
            None
        } else if self.check_keyword(Keyword::Var) {
            Some(Box::new(self.parse_var()?))
        } else if self.looks_like_typed_declaration() {
            Some(Box::new(self.parse_typed_let()?))
        } else {
            let expr = self.parse_expr()?;
            self.expect(TokenKind::Semi, "`;` after the initialiser")?;
            Some(Box::new(Stmt::Expr(expr)))
        };

        let condition = if self.check(&TokenKind::Semi) {
            None
        } else {
            Some(self.parse_expr()?)
        };
        self.expect(TokenKind::Semi, "`;` after the condition")?;

        let step = if self.check(&TokenKind::RParen) {
            None
        } else {
            Some(self.parse_expr()?)
        };
        self.expect(TokenKind::RParen, "`)` after the step")?;

        let body = self.parse_block_or_statement()?;
        let span = start.to(body.span);
        Ok(Stmt::For {
            init,
            condition,
            step,
            body,
            span,
        })
    }

    pub(super) fn parse_foreach(&mut self) -> Parse<Stmt> {
        let start = self.span();
        self.advance();
        self.expect(TokenKind::LParen, "`(` after `foreach`")?;

        let ty = if self.eat_keyword(Keyword::Var) {
            None
        } else {
            Some(self.parse_type()?)
        };
        let (name, _) = self.expect_ident("a loop variable")?;

        if !self.eat_keyword(Keyword::In) {
            return Err(self.error_here("expected `in` after the loop variable"));
        }

        let iterable = self.parse_expr()?;
        self.expect(TokenKind::RParen, "`)` after the sequence")?;
        let body = self.parse_block_or_statement()?;
        let span = start.to(body.span);

        Ok(Stmt::Foreach {
            ty,
            name,
            iterable,
            body,
            span,
        })
    }

    pub(super) fn parse_return(&mut self) -> Parse<Stmt> {
        let start = self.span();
        self.advance();
        let value = if self.check(&TokenKind::Semi) {
            None
        } else {
            Some(self.parse_expr()?)
        };
        let end = self.expect(TokenKind::Semi, "`;` after `return`")?;
        Ok(Stmt::Return {
            value,
            span: start.to(end),
        })
    }

    pub(super) fn parse_become(&mut self) -> Parse<Stmt> {
        let start = self.span();
        self.advance();
        let (state, _) = self.expect_ident("a state name after `become`")?;

        let mut args = Vec::new();
        if self.eat(&TokenKind::LParen) {
            if !self.check(&TokenKind::RParen) {
                loop {
                    args.push(self.parse_expr()?);
                    if !self.eat(&TokenKind::Comma) {
                        break;
                    }
                }
            }
            self.expect(TokenKind::RParen, "`)` after the state arguments")?;
        }

        let end = self.expect(TokenKind::Semi, "`;` after `become`")?;
        Ok(Stmt::Become {
            state,
            args,
            span: start.to(end),
        })
    }

    pub(super) fn parse_match(&mut self) -> Parse<Stmt> {
        let start = self.span();
        self.advance();
        self.expect(TokenKind::LParen, "`(` after `match`")?;
        let subject = self.parse_expr()?;
        self.expect(TokenKind::RParen, "`)` after the subject")?;
        self.expect(TokenKind::LBrace, "`{` to open the match arms")?;

        let mut arms = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.at_end() {
            match self.parse_match_arm() {
                Ok(arm) => arms.push(arm),
                Err(Bail) => self.recover_to_statement(),
            }
        }

        let end = self.expect(TokenKind::RBrace, "`}` to close the match arms")?;
        Ok(Stmt::Match {
            subject,
            arms,
            span: start.to(end),
        })
    }

    pub(super) fn parse_match_arm(&mut self) -> Parse<MatchArm> {
        let start = self.span();
        let pattern = self.parse_pattern()?;

        if !self.eat(&TokenKind::FatArrow) {
            return Err(self.error_here("expected `=>` after the pattern"));
        }

        let body = if self.check(&TokenKind::LBrace) {
            self.parse_block()?
        } else {
            let statement = self.parse_statement_for_arm()?;
            let span = statement.span();
            Block {
                statements: vec![statement],
                span,
            }
        };

        // A trailing comma between arms is allowed, as in the examples.
        self.eat(&TokenKind::Comma);
        let span = start.to(body.span);
        Ok(MatchArm {
            pattern,
            body,
            span,
        })
    }

    /// An arm body that is a bare expression has no `;` — the comma or the
    /// closing brace ends it.
    pub(super) fn parse_statement_for_arm(&mut self) -> Parse<Stmt> {
        if matches!(
            self.peek(),
            TokenKind::Keyword(
                Keyword::Become | Keyword::Return | Keyword::Break | Keyword::Continue
            )
        ) {
            return self.parse_statement();
        }
        let expr = self.parse_expr()?;
        Ok(Stmt::Expr(expr))
    }

    pub(super) fn parse_pattern(&mut self) -> Parse<Pattern> {
        if self.check_keyword(Keyword::Null) {
            let span = self.span();
            self.advance();
            return Ok(Pattern::Null(span));
        }
        if let TokenKind::Ident(name) = self.peek().clone() {
            if name == "_" {
                let span = self.span();
                self.advance();
                return Ok(Pattern::Wildcard(span));
            }
        }

        let start = self.span();
        let ty = self.parse_type()?;
        let (name, name_span) = self.expect_ident("a name to bind the matched value to")?;
        Ok(Pattern::Binding {
            ty,
            name,
            span: start.to(name_span),
        })
    }
}

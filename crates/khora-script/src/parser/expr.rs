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

//! Parsing expressions.
//!
//! Precedence climbs from assignment up to the primaries, mirroring C# so a
//! reader's expectations transfer intact.

use super::{Parse, Parser};
use crate::ast::*;
use crate::lexer::{Keyword, TokenKind};

impl Parser {
    // ── Expressions ───────────────────────────────────
    //
    // Precedence climbs from assignment up to the primaries, mirroring C# so a
    // reader's expectations transfer intact.

    pub(super) fn parse_expr(&mut self) -> Parse<Expr> {
        self.parse_assignment()
    }

    pub(super) fn parse_assignment(&mut self) -> Parse<Expr> {
        let target = self.parse_ternary()?;

        let op = match self.peek() {
            TokenKind::Assign => None,
            TokenKind::PlusAssign => Some(BinaryOp::Add),
            TokenKind::MinusAssign => Some(BinaryOp::Sub),
            TokenKind::StarAssign => Some(BinaryOp::Mul),
            TokenKind::SlashAssign => Some(BinaryOp::Div),
            _ => return Ok(target),
        };
        self.advance();

        // Right-associative: `a = b = c` is `a = (b = c)`.
        let value = self.parse_assignment()?;
        let span = target.span().to(value.span());
        Ok(Expr::Assign {
            target: Box::new(target),
            op,
            value: Box::new(value),
            span,
        })
    }

    pub(super) fn parse_ternary(&mut self) -> Parse<Expr> {
        let condition = self.parse_coalesce()?;
        if !self.check(&TokenKind::Question) {
            return Ok(condition);
        }
        self.advance();

        let then_value = self.parse_expr()?;
        self.expect(TokenKind::Colon, "`:` in the conditional")?;
        let else_value = self.parse_ternary()?;
        let span = condition.span().to(else_value.span());

        Ok(Expr::Ternary {
            condition: Box::new(condition),
            then_value: Box::new(then_value),
            else_value: Box::new(else_value),
            span,
        })
    }

    pub(super) fn parse_coalesce(&mut self) -> Parse<Expr> {
        let mut lhs = self.parse_or()?;
        while self.check(&TokenKind::QuestionQuestion) {
            self.advance();
            let rhs = self.parse_or()?;
            let span = lhs.span().to(rhs.span());
            lhs = Expr::Binary {
                op: BinaryOp::Coalesce,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span,
            };
        }
        Ok(lhs)
    }

    pub(super) fn parse_or(&mut self) -> Parse<Expr> {
        let mut lhs = self.parse_and()?;
        while self.check(&TokenKind::OrOr) {
            self.advance();
            let rhs = self.parse_and()?;
            let span = lhs.span().to(rhs.span());
            lhs = Expr::Binary {
                op: BinaryOp::Or,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span,
            };
        }
        Ok(lhs)
    }

    pub(super) fn parse_and(&mut self) -> Parse<Expr> {
        let mut lhs = self.parse_equality()?;
        while self.check(&TokenKind::AndAnd) {
            self.advance();
            let rhs = self.parse_equality()?;
            let span = lhs.span().to(rhs.span());
            lhs = Expr::Binary {
                op: BinaryOp::And,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span,
            };
        }
        Ok(lhs)
    }

    pub(super) fn parse_equality(&mut self) -> Parse<Expr> {
        let mut lhs = self.parse_comparison()?;
        loop {
            let op = match self.peek() {
                TokenKind::Eq => BinaryOp::Eq,
                TokenKind::NotEq => BinaryOp::NotEq,
                _ => return Ok(lhs),
            };
            self.advance();
            let rhs = self.parse_comparison()?;
            let span = lhs.span().to(rhs.span());
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span,
            };
        }
    }

    pub(super) fn parse_comparison(&mut self) -> Parse<Expr> {
        let mut lhs = self.parse_additive()?;
        loop {
            let op = match self.peek() {
                TokenKind::Less => BinaryOp::Less,
                TokenKind::LessEq => BinaryOp::LessEq,
                TokenKind::Greater => BinaryOp::Greater,
                TokenKind::GreaterEq => BinaryOp::GreaterEq,
                _ => return Ok(lhs),
            };
            self.advance();
            let rhs = self.parse_additive()?;
            let span = lhs.span().to(rhs.span());
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span,
            };
        }
    }

    pub(super) fn parse_additive(&mut self) -> Parse<Expr> {
        let mut lhs = self.parse_multiplicative()?;
        loop {
            let op = match self.peek() {
                TokenKind::Plus => BinaryOp::Add,
                TokenKind::Minus => BinaryOp::Sub,
                _ => return Ok(lhs),
            };
            self.advance();
            let rhs = self.parse_multiplicative()?;
            let span = lhs.span().to(rhs.span());
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span,
            };
        }
    }

    pub(super) fn parse_multiplicative(&mut self) -> Parse<Expr> {
        let mut lhs = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                TokenKind::Star => BinaryOp::Mul,
                TokenKind::Slash => BinaryOp::Div,
                TokenKind::Percent => BinaryOp::Rem,
                _ => return Ok(lhs),
            };
            self.advance();
            let rhs = self.parse_unary()?;
            let span = lhs.span().to(rhs.span());
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span,
            };
        }
    }

    pub(super) fn parse_unary(&mut self) -> Parse<Expr> {
        let start = self.span();

        if self.check(&TokenKind::Minus) {
            self.advance();
            let operand = self.parse_unary()?;
            let span = start.to(operand.span());
            return Ok(Expr::Unary {
                op: UnaryOp::Neg,
                operand: Box::new(operand),
                span,
            });
        }
        if self.check(&TokenKind::Bang) {
            self.advance();
            let operand = self.parse_unary()?;
            let span = start.to(operand.span());
            return Ok(Expr::Unary {
                op: UnaryOp::Not,
                operand: Box::new(operand),
                span,
            });
        }
        if self.check_keyword(Keyword::Await) {
            self.advance();
            let operand = self.parse_unary()?;
            let span = start.to(operand.span());
            return Ok(Expr::Await {
                operand: Box::new(operand),
                span,
            });
        }

        self.parse_postfix()
    }

    pub(super) fn parse_postfix(&mut self) -> Parse<Expr> {
        let mut expr = self.parse_primary()?;

        loop {
            if self.check(&TokenKind::LParen) {
                self.advance();
                let mut args = Vec::new();
                if !self.check(&TokenKind::RParen) {
                    loop {
                        args.push(self.parse_expr()?);
                        if !self.eat(&TokenKind::Comma) {
                            break;
                        }
                    }
                }
                let end = self.expect(TokenKind::RParen, "`)` after the arguments")?;
                let span = expr.span().to(end);
                expr = Expr::Call {
                    callee: Box::new(expr),
                    args,
                    span,
                };
                continue;
            }

            if self.check(&TokenKind::Dot) {
                self.advance();
                let (name, name_span) = self.expect_ident("a field or method name")?;
                let span = expr.span().to(name_span);
                expr = Expr::Field {
                    object: Box::new(expr),
                    name,
                    span,
                };
                continue;
            }

            if self.check(&TokenKind::QuestionDot) {
                self.advance();
                let (name, name_span) = self.expect_ident("a field or method name")?;
                let span = expr.span().to(name_span);
                expr = Expr::OptionalField {
                    object: Box::new(expr),
                    name,
                    span,
                };
                continue;
            }

            if self.check(&TokenKind::LBracket) {
                self.advance();
                let index = self.parse_expr()?;
                let end = self.expect(TokenKind::RBracket, "`]` after the index")?;
                let span = expr.span().to(end);
                expr = Expr::Index {
                    object: Box::new(expr),
                    index: Box::new(index),
                    span,
                };
                continue;
            }

            return Ok(expr);
        }
    }

    pub(super) fn parse_primary(&mut self) -> Parse<Expr> {
        let span = self.span();

        match self.peek().clone() {
            TokenKind::Int(value) => {
                self.advance();
                Ok(Expr::Int { value, span })
            }
            TokenKind::Float(value) => {
                self.advance();
                Ok(Expr::Float { value, span })
            }
            TokenKind::Str(value) => {
                self.advance();
                Ok(Expr::Str { value, span })
            }
            TokenKind::Duration(value) => {
                self.advance();
                Ok(Expr::Duration { value, span })
            }
            TokenKind::Angle(value) => {
                self.advance();
                Ok(Expr::Angle { value, span })
            }
            TokenKind::Keyword(Keyword::True) => {
                self.advance();
                Ok(Expr::Bool { value: true, span })
            }
            TokenKind::Keyword(Keyword::False) => {
                self.advance();
                Ok(Expr::Bool { value: false, span })
            }
            TokenKind::Keyword(Keyword::Null) => {
                self.advance();
                Ok(Expr::Null(span))
            }
            TokenKind::Keyword(Keyword::This) => {
                self.advance();
                Ok(Expr::This(span))
            }
            TokenKind::Ident(name) => {
                self.advance();
                Ok(Expr::Ident { name, span })
            }
            TokenKind::Keyword(Keyword::New) => {
                self.advance();
                let ty = self.parse_type()?;
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
                    self.expect(TokenKind::RParen, "`)` after the arguments")?;
                }
                let end = self.previous_span();
                Ok(Expr::New {
                    ty,
                    args,
                    span: span.to(end),
                })
            }
            TokenKind::LBracket => {
                self.advance();
                let mut elements = Vec::new();
                if !self.check(&TokenKind::RBracket) {
                    loop {
                        elements.push(self.parse_expr()?);
                        if !self.eat(&TokenKind::Comma) {
                            break;
                        }
                    }
                }
                let end = self.expect(TokenKind::RBracket, "`]` to close the array")?;
                Ok(Expr::ArrayLit {
                    elements,
                    span: span.to(end),
                })
            }
            TokenKind::LParen => {
                // `(Type)expr` is a cast; anything else is grouping. The two
                // are told apart by what follows the closing paren.
                if self.looks_like_cast() {
                    self.advance();
                    let ty = self.parse_type()?;
                    self.expect(TokenKind::RParen, "`)` after the cast type")?;
                    let operand = self.parse_unary()?;
                    let full = span.to(operand.span());
                    return Ok(Expr::Cast {
                        ty,
                        operand: Box::new(operand),
                        span: full,
                    });
                }
                self.advance();
                let inner = self.parse_expr()?;
                self.expect(TokenKind::RParen, "`)` to close the group")?;
                Ok(inner)
            }
            _ => Err(self.error_here("expected an expression")),
        }
    }

    /// Whether `(` opens a cast rather than a group.
    ///
    /// Only the built-in numeric conversions are treated as casts. Widening the
    /// rule would make `(x)` ambiguous with a cast to a user type named `x`,
    /// and gameplay code groups far more often than it converts.
    pub(super) fn looks_like_cast(&self) -> bool {
        let TokenKind::Ident(name) = self.peek_at(1) else {
            return false;
        };
        matches!(name.as_str(), "int" | "float") && matches!(self.peek_at(2), TokenKind::RParen)
    }
}

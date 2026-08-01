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

//! Parsing declarations: imports, structs, functions, behaviors and their
//! members.

use super::describe::describe;
use super::{Bail, Parse, Parser};
use crate::ast::*;
use crate::diagnostics::Diagnostic;
use crate::lexer::{Keyword, TokenKind};

impl Parser {
    // ── Declarations ──────────────────────────────────

    pub(super) fn parse_import(&mut self) -> Parse<Import> {
        let start = self.span();
        self.advance(); // `import`

        let TokenKind::Str(path) = self.peek().clone() else {
            return Err(self.error_with_note(
                "expected a quoted path after `import`",
                "paths are relative to the project's script root, e.g. `import \"ai/steering.erg\";`",
            ));
        };
        self.advance();

        let alias = if self.eat_keyword(Keyword::As) {
            Some(self.expect_ident("an alias after `as`")?.0)
        } else {
            None
        };

        let end = self.expect(TokenKind::Semi, "`;` after the import")?;
        Ok(Import {
            path,
            alias,
            span: start.to(end),
        })
    }

    pub(super) fn parse_item(&mut self) -> Parse<Item> {
        // Attributes bind to the declaration that follows, so they are read
        // before deciding what that declaration is.
        let attributes = self.parse_attributes()?;

        if self.check_keyword(Keyword::Behavior) {
            return Ok(Item::Behavior(self.parse_behavior(attributes)?));
        }

        if let Some(attribute) = attributes.first() {
            self.diagnostics.push(
                Diagnostic::error("attributes only apply to behaviors", attribute.span)
                    .with_note("`[Critical]`, `[Budget(…)]` and `[Rate(…)]` guide the DCC's arbitration, which schedules behaviors"),
            );
        }

        if self.check_keyword(Keyword::Struct) {
            return Ok(Item::Struct(self.parse_struct()?));
        }
        if self.check_keyword(Keyword::Fn) {
            return Ok(Item::Function(self.parse_function()?));
        }

        Err(self.error_with_note(
            "expected a declaration",
            "a file may declare `behavior`, `struct` or `fn`, and start with `import`",
        ))
    }

    pub(super) fn parse_attributes(&mut self) -> Parse<Vec<Attribute>> {
        let mut attributes = Vec::new();
        while self.check(&TokenKind::LBracket) {
            let start = self.span();
            self.advance();
            let (name, _) = self.expect_ident("an attribute name")?;

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
                self.expect(TokenKind::RParen, "`)` after the attribute arguments")?;
            }

            let end = self.expect(TokenKind::RBracket, "`]` after the attribute")?;
            attributes.push(Attribute {
                name,
                args,
                span: start.to(end),
            });
        }
        Ok(attributes)
    }

    pub(super) fn parse_struct(&mut self) -> Parse<StructDecl> {
        let start = self.span();
        self.advance(); // `struct`
        let (name, name_span) = self.expect_ident("a struct name")?;
        self.expect(TokenKind::LBrace, "`{` to open the struct body")?;

        let mut fields = Vec::new();
        let mut operators = Vec::new();

        while !self.check(&TokenKind::RBrace) && !self.at_end() {
            if self.check_keyword(Keyword::Static) {
                match self.parse_operator() {
                    Ok(op) => operators.push(op),
                    Err(Bail) => self.recover_to_statement(),
                }
                continue;
            }
            match self.parse_field() {
                Ok(field) => fields.push(field),
                Err(Bail) => self.recover_to_statement(),
            }
        }

        let end = self.expect(TokenKind::RBrace, "`}` to close the struct body")?;
        Ok(StructDecl {
            name,
            name_span,
            fields,
            operators,
            span: start.to(end),
        })
    }

    pub(super) fn parse_operator(&mut self) -> Parse<OperatorDecl> {
        let start = self.span();
        self.advance(); // `static`
        let return_ty = self.parse_type()?;

        if !self.eat_keyword(Keyword::Operator) {
            return Err(self.error_with_note(
                "expected `operator`",
                "a `static` member of a struct declares an operator overload, e.g. `static Health operator -(Health h, int damage)`",
            ));
        }

        let op_span = self.span();
        let op = match self.advance() {
            TokenKind::Plus => OverloadableOp::Add,
            TokenKind::Minus => OverloadableOp::Sub,
            TokenKind::Star => OverloadableOp::Mul,
            TokenKind::Slash => OverloadableOp::Div,
            TokenKind::Percent => OverloadableOp::Rem,
            TokenKind::Eq => OverloadableOp::Eq,
            TokenKind::NotEq => OverloadableOp::NotEq,
            TokenKind::Less => OverloadableOp::Less,
            TokenKind::LessEq => OverloadableOp::LessEq,
            TokenKind::Greater => OverloadableOp::Greater,
            TokenKind::GreaterEq => OverloadableOp::GreaterEq,
            other => {
                let found = describe(&other);
                self.diagnostics.push(
                    Diagnostic::error(format!("`{found}` cannot be overloaded"), op_span).with_note(
                        "overloadable: + - * / % == != < <= > >= — `&&`, `||` and `!` are not, so short-circuiting stays predictable",
                    ),
                );
                return Err(Bail);
            }
        };

        let params = self.parse_params()?;
        let body = self.parse_body_or_arrow(true)?;
        let span = start.to(body.span);
        Ok(OperatorDecl {
            op,
            op_span,
            return_ty,
            params,
            body,
            span,
        })
    }

    pub(super) fn parse_function(&mut self) -> Parse<FunctionDecl> {
        let start = self.span();
        self.advance(); // `fn`
        let return_ty = self.parse_type()?;
        let (name, name_span) = self.expect_ident("a function name")?;
        let params = self.parse_params()?;
        let body = self.parse_block()?;
        let span = start.to(body.span);
        Ok(FunctionDecl {
            name,
            name_span,
            return_ty,
            params,
            body,
            span,
        })
    }

    pub(super) fn parse_behavior(&mut self, attributes: Vec<Attribute>) -> Parse<BehaviorDecl> {
        let start = self.span();
        self.advance(); // `behavior`
        let (name, name_span) = self.expect_ident("a behavior name")?;
        self.expect(TokenKind::LBrace, "`{` to open the behavior body")?;
        let members = self.parse_members();
        let end = self.expect(TokenKind::RBrace, "`}` to close the behavior body")?;

        Ok(BehaviorDecl {
            attributes,
            name,
            name_span,
            members,
            span: start.to(end),
        })
    }

    /// Members of a `behavior` or a `state`. Never bails: a broken member is
    /// skipped so the ones after it are still parsed and checked.
    pub(super) fn parse_members(&mut self) -> Vec<BehaviorMember> {
        let mut members = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.at_end() {
            match self.parse_member() {
                Ok(member) => members.push(member),
                Err(Bail) => self.recover_to_statement(),
            }
        }
        members
    }

    pub(super) fn parse_member(&mut self) -> Parse<BehaviorMember> {
        if self.check_keyword(Keyword::State) {
            return Ok(BehaviorMember::State(self.parse_state()?));
        }
        if self.check_keyword(Keyword::On) {
            return Ok(BehaviorMember::Handler(self.parse_handler()?));
        }
        if self.check_keyword(Keyword::Every) {
            return Ok(BehaviorMember::Every(self.parse_every()?));
        }
        if self.check_keyword(Keyword::After) {
            return Ok(BehaviorMember::After(self.parse_after()?));
        }
        if self.check_keyword(Keyword::Async) {
            return Ok(BehaviorMember::Method(self.parse_method(true)?));
        }

        // A field and a method both open with a type and a name; the token
        // after tells them apart.
        if self.looks_like_method() {
            return Ok(BehaviorMember::Method(self.parse_method(false)?));
        }
        Ok(BehaviorMember::Field(self.parse_field()?))
    }

    /// Whether what follows is `Type Name(` rather than `Type Name;`.
    pub(super) fn looks_like_method(&self) -> bool {
        let mut ahead = 0;

        // Skip the type, including `[]` and `?` suffixes and `<…>` arguments.
        if !matches!(
            self.peek(),
            TokenKind::Ident(_) | TokenKind::Keyword(Keyword::Void)
        ) {
            return false;
        }
        ahead += 1;

        loop {
            match self.peek_at(ahead) {
                TokenKind::LBracket if matches!(self.peek_at(ahead + 1), TokenKind::RBracket) => {
                    ahead += 2;
                }
                TokenKind::Question => ahead += 1,
                TokenKind::Less => {
                    // Walk to the matching `>`; nesting is shallow in practice
                    // and a runaway is bounded by the end of input.
                    let mut depth = 0;
                    loop {
                        match self.peek_at(ahead) {
                            TokenKind::Less => depth += 1,
                            TokenKind::Greater => {
                                depth -= 1;
                                if depth == 0 {
                                    ahead += 1;
                                    break;
                                }
                            }
                            TokenKind::Eof => return false,
                            _ => {}
                        }
                        ahead += 1;
                    }
                }
                _ => break,
            }
        }

        matches!(self.peek_at(ahead), TokenKind::Ident(_))
            && matches!(self.peek_at(ahead + 1), TokenKind::LParen)
    }

    pub(super) fn parse_state(&mut self) -> Parse<StateDecl> {
        let start = self.span();
        self.advance(); // `state`
        let (name, name_span) = self.expect_ident("a state name")?;

        let params = if self.check(&TokenKind::LParen) {
            self.parse_params()?
        } else {
            Vec::new()
        };

        self.expect(TokenKind::LBrace, "`{` to open the state body")?;
        let members = self.parse_members();
        let end = self.expect(TokenKind::RBrace, "`}` to close the state body")?;

        Ok(StateDecl {
            name,
            name_span,
            params,
            members,
            span: start.to(end),
        })
    }

    pub(super) fn parse_handler(&mut self) -> Parse<HandlerDecl> {
        let start = self.span();
        self.advance(); // `on`
        let (event, event_span) = self.expect_ident("an event name after `on`")?;

        let params = if self.check(&TokenKind::LParen) {
            self.parse_params()?
        } else {
            Vec::new()
        };

        let body = self.parse_body_or_arrow(false)?;
        let span = start.to(body.span);
        Ok(HandlerDecl {
            event,
            event_span,
            params,
            body,
            span,
        })
    }

    pub(super) fn parse_every(&mut self) -> Parse<EveryDecl> {
        let start = self.span();
        self.advance(); // `every`
        let interval = self.parse_expr()?;
        let body = self.parse_body_or_arrow(false)?;
        let span = start.to(body.span);
        Ok(EveryDecl {
            interval,
            body,
            span,
        })
    }

    pub(super) fn parse_after(&mut self) -> Parse<AfterDecl> {
        let start = self.span();
        self.advance(); // `after`
        let delay = self.parse_expr()?;
        let body = self.parse_body_or_arrow(false)?;
        let span = start.to(body.span);
        Ok(AfterDecl { delay, body, span })
    }

    /// `{ … }` or `=> …;`.
    ///
    /// The arrow form becomes a one-statement block, so everything downstream
    /// sees a single shape and no pass has to special-case the shorthand.
    ///
    /// `returns_value` decides what that statement is. On a member that
    /// produces a value — an operator, a non-`void` method — `=> expr` means
    /// *return* `expr`; on a `void` one it is an expression evaluated for
    /// effect. Getting this wrong would silently discard the result of every
    /// arrow-bodied operator.
    pub(super) fn parse_body_or_arrow(&mut self, returns_value: bool) -> Parse<Block> {
        if self.eat(&TokenKind::FatArrow) {
            let arrow = self.previous_span();

            // A statement keyword after the arrow is taken as written:
            // `on Lost => become Patrol;` is a transition, not a value.
            if !returns_value || self.starts_statement() {
                let statement = self.parse_statement()?;
                let span = arrow.to(statement.span());
                return Ok(Block {
                    statements: vec![statement],
                    span,
                });
            }

            let value = self.parse_expr()?;
            let end = self.expect(TokenKind::Semi, "`;` after the expression body")?;
            let span = arrow.to(end);
            return Ok(Block {
                statements: vec![Stmt::Return {
                    value: Some(value),
                    span,
                }],
                span,
            });
        }
        self.parse_block()
    }

    /// Whether the next token can only begin a statement, never an expression.
    pub(super) fn starts_statement(&self) -> bool {
        matches!(
            self.peek(),
            TokenKind::Keyword(
                Keyword::Become
                    | Keyword::Return
                    | Keyword::Break
                    | Keyword::Continue
                    | Keyword::If
                    | Keyword::While
                    | Keyword::For
                    | Keyword::Foreach
                    | Keyword::Match
            )
        )
    }

    pub(super) fn parse_method(&mut self, is_async: bool) -> Parse<MethodDecl> {
        let start = self.span();
        if is_async {
            self.advance(); // `async`
        }
        let return_ty = self.parse_type()?;
        let (name, name_span) = self.expect_ident("a method name")?;
        let params = self.parse_params()?;
        let body = self.parse_body_or_arrow(!matches!(return_ty, TypeRef::Void))?;
        let span = start.to(body.span);
        Ok(MethodDecl {
            is_async,
            return_ty,
            name,
            name_span,
            params,
            body,
            span,
        })
    }

    pub(super) fn parse_field(&mut self) -> Parse<Field> {
        let start = self.span();
        let ty = self.parse_type()?;
        let (name, _) = self.expect_ident("a field name")?;

        let default = if self.eat(&TokenKind::Assign) {
            Some(self.parse_expr()?)
        } else {
            None
        };

        let end = self.expect(TokenKind::Semi, "`;` after the field")?;
        Ok(Field {
            ty,
            name,
            default,
            span: start.to(end),
        })
    }

    pub(super) fn parse_params(&mut self) -> Parse<Vec<Param>> {
        self.expect(TokenKind::LParen, "`(` to open the parameter list")?;
        let mut params = Vec::new();

        if !self.check(&TokenKind::RParen) {
            loop {
                let start = self.span();
                let ty = self.parse_type()?;
                let (name, name_span) = self.expect_ident("a parameter name")?;
                params.push(Param {
                    ty,
                    name,
                    span: start.to(name_span),
                });
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
        }

        self.expect(TokenKind::RParen, "`)` to close the parameter list")?;
        Ok(params)
    }
}

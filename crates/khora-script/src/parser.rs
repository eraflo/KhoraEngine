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
        self.diagnostics
            .iter()
            .any(|d| d.severity == crate::diagnostics::Severity::Error)
    }
}

/// Parses `tokens` into a module.
pub fn parse(tokens: Vec<Token>) -> Parsed {
    Parser::new(tokens).run()
}

/// Signals that the parser gave up on the current construct and should recover.
/// The diagnostic is already recorded when this travels.
struct Bail;

type Parse<T> = Result<T, Bail>;

struct Parser {
    tokens: Vec<Token>,
    position: usize,
    diagnostics: Vec<Diagnostic>,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            position: 0,
            diagnostics: Vec::new(),
        }
    }

    fn run(mut self) -> Parsed {
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

    fn peek(&self) -> &TokenKind {
        self.tokens
            .get(self.position)
            .map(|t| &t.kind)
            .unwrap_or(&TokenKind::Eof)
    }

    fn peek_at(&self, ahead: usize) -> &TokenKind {
        self.tokens
            .get(self.position + ahead)
            .map(|t| &t.kind)
            .unwrap_or(&TokenKind::Eof)
    }

    fn span(&self) -> Span {
        self.tokens
            .get(self.position)
            .map(|t| t.span)
            .unwrap_or_else(|| self.tokens.last().map(|t| t.span).unwrap_or(Span::empty(0)))
    }

    fn previous_span(&self) -> Span {
        self.tokens
            .get(self.position.saturating_sub(1))
            .map(|t| t.span)
            .unwrap_or(Span::empty(0))
    }

    fn at_end(&self) -> bool {
        matches!(self.peek(), TokenKind::Eof)
    }

    fn advance(&mut self) -> TokenKind {
        let kind = self.peek().clone();
        if !self.at_end() {
            self.position += 1;
        }
        kind
    }

    fn check(&self, kind: &TokenKind) -> bool {
        self.peek() == kind
    }

    fn check_keyword(&self, keyword: Keyword) -> bool {
        matches!(self.peek(), TokenKind::Keyword(k) if *k == keyword)
    }

    fn eat(&mut self, kind: &TokenKind) -> bool {
        if self.check(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn eat_keyword(&mut self, keyword: Keyword) -> bool {
        if self.check_keyword(keyword) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, kind: TokenKind, what: &str) -> Parse<Span> {
        if self.check(&kind) {
            let span = self.span();
            self.advance();
            return Ok(span);
        }
        Err(self.error_here(format!("expected {what}")))
    }

    fn expect_ident(&mut self, what: &str) -> Parse<(String, Span)> {
        if let TokenKind::Ident(name) = self.peek().clone() {
            let span = self.span();
            self.advance();
            return Ok((name, span));
        }
        Err(self.error_here(format!("expected {what}")))
    }

    /// Records an error at the current token and signals a bail.
    fn error_here(&mut self, message: impl Into<String>) -> Bail {
        let found = describe(self.peek());
        let span = self.span();
        self.diagnostics.push(Diagnostic::error(
            format!("{}, found {found}", message.into()),
            span,
        ));
        Bail
    }

    fn error_with_note(&mut self, message: impl Into<String>, note: impl Into<String>) -> Bail {
        let found = describe(self.peek());
        let span = self.span();
        self.diagnostics.push(
            Diagnostic::error(format!("{}, found {found}", message.into()), span).with_note(note),
        );
        Bail
    }

    // ── Recovery ──────────────────────────────────────

    /// Skips to something that can begin a top-level declaration.
    fn recover_to_item(&mut self) {
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
    fn recover_to_statement(&mut self) {
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

    // ── Declarations ──────────────────────────────────

    fn parse_import(&mut self) -> Parse<Import> {
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

    fn parse_item(&mut self) -> Parse<Item> {
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

    fn parse_attributes(&mut self) -> Parse<Vec<Attribute>> {
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

    fn parse_struct(&mut self) -> Parse<StructDecl> {
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

    fn parse_operator(&mut self) -> Parse<OperatorDecl> {
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

    fn parse_function(&mut self) -> Parse<FunctionDecl> {
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

    fn parse_behavior(&mut self, attributes: Vec<Attribute>) -> Parse<BehaviorDecl> {
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
    fn parse_members(&mut self) -> Vec<BehaviorMember> {
        let mut members = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.at_end() {
            match self.parse_member() {
                Ok(member) => members.push(member),
                Err(Bail) => self.recover_to_statement(),
            }
        }
        members
    }

    fn parse_member(&mut self) -> Parse<BehaviorMember> {
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
    fn looks_like_method(&self) -> bool {
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

    fn parse_state(&mut self) -> Parse<StateDecl> {
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

    fn parse_handler(&mut self) -> Parse<HandlerDecl> {
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

    fn parse_every(&mut self) -> Parse<EveryDecl> {
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

    fn parse_after(&mut self) -> Parse<AfterDecl> {
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
    fn parse_body_or_arrow(&mut self, returns_value: bool) -> Parse<Block> {
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
    fn starts_statement(&self) -> bool {
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

    fn parse_method(&mut self, is_async: bool) -> Parse<MethodDecl> {
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

    fn parse_field(&mut self) -> Parse<Field> {
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

    fn parse_params(&mut self) -> Parse<Vec<Param>> {
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

    // ── Types ─────────────────────────────────────────

    fn parse_type(&mut self) -> Parse<TypeRef> {
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

    // ── Statements ────────────────────────────────────

    fn parse_block(&mut self) -> Parse<Block> {
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

    fn parse_statement(&mut self) -> Parse<Stmt> {
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
    fn looks_like_typed_declaration(&self) -> bool {
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

    fn parse_typed_let(&mut self) -> Parse<Stmt> {
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

    fn parse_var(&mut self) -> Parse<Stmt> {
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

    fn parse_if(&mut self) -> Parse<Stmt> {
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
    fn parse_block_or_statement(&mut self) -> Parse<Block> {
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

    fn parse_while(&mut self) -> Parse<Stmt> {
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

    fn parse_for(&mut self) -> Parse<Stmt> {
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

    fn parse_foreach(&mut self) -> Parse<Stmt> {
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

    fn parse_return(&mut self) -> Parse<Stmt> {
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

    fn parse_become(&mut self) -> Parse<Stmt> {
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

    fn parse_match(&mut self) -> Parse<Stmt> {
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

    fn parse_match_arm(&mut self) -> Parse<MatchArm> {
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
    fn parse_statement_for_arm(&mut self) -> Parse<Stmt> {
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

    fn parse_pattern(&mut self) -> Parse<Pattern> {
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

    // ── Expressions ───────────────────────────────────
    //
    // Precedence climbs from assignment up to the primaries, mirroring C# so a
    // reader's expectations transfer intact.

    fn parse_expr(&mut self) -> Parse<Expr> {
        self.parse_assignment()
    }

    fn parse_assignment(&mut self) -> Parse<Expr> {
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

    fn parse_ternary(&mut self) -> Parse<Expr> {
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

    fn parse_coalesce(&mut self) -> Parse<Expr> {
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

    fn parse_or(&mut self) -> Parse<Expr> {
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

    fn parse_and(&mut self) -> Parse<Expr> {
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

    fn parse_equality(&mut self) -> Parse<Expr> {
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

    fn parse_comparison(&mut self) -> Parse<Expr> {
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

    fn parse_additive(&mut self) -> Parse<Expr> {
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

    fn parse_multiplicative(&mut self) -> Parse<Expr> {
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

    fn parse_unary(&mut self) -> Parse<Expr> {
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

    fn parse_postfix(&mut self) -> Parse<Expr> {
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

    fn parse_primary(&mut self) -> Parse<Expr> {
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
    fn looks_like_cast(&self) -> bool {
        let TokenKind::Ident(name) = self.peek_at(1) else {
            return false;
        };
        matches!(name.as_str(), "int" | "float") && matches!(self.peek_at(2), TokenKind::RParen)
    }
}

impl Stmt {
    /// Where the statement was written.
    pub fn span(&self) -> Span {
        match self {
            Self::Let { span, .. }
            | Self::If { span, .. }
            | Self::While { span, .. }
            | Self::For { span, .. }
            | Self::Foreach { span, .. }
            | Self::Return { span, .. }
            | Self::Become { span, .. }
            | Self::Match { span, .. } => *span,
            Self::Break(span) | Self::Continue(span) => *span,
            Self::Expr(expr) => expr.span(),
            Self::Block(block) => block.span,
        }
    }
}

/// A human name for a token, for "expected X, found Y".
fn describe(kind: &TokenKind) -> String {
    match kind {
        TokenKind::Int(v) => format!("`{v}`"),
        TokenKind::Float(v) => format!("`{v}`"),
        TokenKind::Str(_) => "a string".to_owned(),
        TokenKind::Duration(v) => format!("`{v}s`"),
        TokenKind::Angle(_) => "an angle".to_owned(),
        TokenKind::Ident(name) => format!("`{name}`"),
        TokenKind::Keyword(k) => format!("`{}`", keyword_text(*k)),
        TokenKind::Eof => "the end of the file".to_owned(),
        other => format!("`{}`", symbol_text(other)),
    }
}

fn keyword_text(keyword: Keyword) -> &'static str {
    match keyword {
        Keyword::Behavior => "behavior",
        Keyword::State => "state",
        Keyword::Become => "become",
        Keyword::On => "on",
        Keyword::Every => "every",
        Keyword::After => "after",
        Keyword::Async => "async",
        Keyword::Await => "await",
        Keyword::Import => "import",
        Keyword::As => "as",
        Keyword::Struct => "struct",
        Keyword::Fn => "fn",
        Keyword::Var => "var",
        Keyword::Void => "void",
        Keyword::Static => "static",
        Keyword::Operator => "operator",
        Keyword::New => "new",
        Keyword::If => "if",
        Keyword::Else => "else",
        Keyword::While => "while",
        Keyword::For => "for",
        Keyword::Foreach => "foreach",
        Keyword::In => "in",
        Keyword::Match => "match",
        Keyword::Return => "return",
        Keyword::Break => "break",
        Keyword::Continue => "continue",
        Keyword::True => "true",
        Keyword::False => "false",
        Keyword::Null => "null",
        Keyword::This => "this",
    }
}

fn symbol_text(kind: &TokenKind) -> &'static str {
    match kind {
        TokenKind::LParen => "(",
        TokenKind::RParen => ")",
        TokenKind::LBrace => "{",
        TokenKind::RBrace => "}",
        TokenKind::LBracket => "[",
        TokenKind::RBracket => "]",
        TokenKind::Comma => ",",
        TokenKind::Semi => ";",
        TokenKind::Dot => ".",
        TokenKind::Colon => ":",
        TokenKind::Assign => "=",
        TokenKind::FatArrow => "=>",
        TokenKind::Plus => "+",
        TokenKind::Minus => "-",
        TokenKind::Star => "*",
        TokenKind::Slash => "/",
        TokenKind::Percent => "%",
        TokenKind::PlusAssign => "+=",
        TokenKind::MinusAssign => "-=",
        TokenKind::StarAssign => "*=",
        TokenKind::SlashAssign => "/=",
        TokenKind::Eq => "==",
        TokenKind::NotEq => "!=",
        TokenKind::Less => "<",
        TokenKind::LessEq => "<=",
        TokenKind::Greater => ">",
        TokenKind::GreaterEq => ">=",
        TokenKind::AndAnd => "&&",
        TokenKind::OrOr => "||",
        TokenKind::Bang => "!",
        TokenKind::Question => "?",
        TokenKind::QuestionQuestion => "??",
        TokenKind::QuestionDot => "?.",
        TokenKind::At => "@",
        _ => "?",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;

    fn parse_ok(source: &str) -> Module {
        let lexed = lex(source);
        assert!(!lexed.has_errors(), "lex errors: {:?}", lexed.diagnostics);
        let parsed = parse(lexed.tokens);
        assert!(
            !parsed.has_errors(),
            "parse errors: {:?}",
            parsed
                .diagnostics
                .iter()
                .map(|d| &d.message)
                .collect::<Vec<_>>()
        );
        parsed.module
    }

    fn parse_errors(source: &str) -> Vec<String> {
        let lexed = lex(source);
        parse(lexed.tokens)
            .diagnostics
            .into_iter()
            .map(|d| d.message)
            .collect()
    }

    /// The example from the design: every construct the language adds over C#
    /// in one behavior.
    #[test]
    fn the_guard_example_parses() {
        let module = parse_ok(
            r#"
            behavior Guard {
                float speed = 3.0;
                int health = 100;

                state Patrol {
                    Vec3[] waypoints;
                    int current = 0;

                    void Update(float dt) {
                        MoveToward(waypoints[current], speed * dt);
                        if (Arrived(waypoints[current]))
                            current = (current + 1) % waypoints.Length;
                    }

                    every 0.5s {
                        become Chase(SeeEnemy());
                    }
                }

                state Chase(Entity prey) {
                    void Update(float dt) => MoveToward(prey.position, speed * 1.5 * dt);

                    on Lost => become Patrol;
                    after 10s => become Patrol;
                }

                on Damaged(int amount) {
                    health -= amount;
                    if (health <= 0) {
                        Spawn(transform.position);
                        Despawn(this);
                    }
                }
            }
            "#,
        );

        assert_eq!(module.items.len(), 1);
        let Item::Behavior(behavior) = &module.items[0] else {
            panic!("expected a behavior");
        };
        assert_eq!(behavior.name, "Guard");

        let states: Vec<_> = behavior
            .members
            .iter()
            .filter_map(|m| match m {
                BehaviorMember::State(state) => Some(state.name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(states, vec!["Patrol", "Chase"]);
    }

    /// A state carries its own data, which is the containment C# cannot offer.
    #[test]
    fn a_state_owns_its_fields_and_parameters() {
        let module = parse_ok("behavior B { state Chase(Entity prey) { int hits = 0; } }");
        let Item::Behavior(behavior) = &module.items[0] else {
            panic!("expected a behavior");
        };
        let BehaviorMember::State(state) = &behavior.members[0] else {
            panic!("expected a state");
        };
        assert_eq!(state.params.len(), 1);
        assert_eq!(state.params[0].name, "prey");
        assert!(matches!(state.members[0], BehaviorMember::Field(_)));
    }

    /// The arrow shorthand becomes a one-statement block, so no later pass has
    /// to know the shorthand exists.
    #[test]
    fn the_arrow_form_is_stored_as_a_block() {
        let module = parse_ok("behavior B { on Lost => become Patrol; }");
        let Item::Behavior(behavior) = &module.items[0] else {
            panic!("expected a behavior");
        };
        let BehaviorMember::Handler(handler) = &behavior.members[0] else {
            panic!("expected a handler");
        };
        assert_eq!(handler.body.statements.len(), 1);
        assert!(matches!(handler.body.statements[0], Stmt::Become { .. }));
    }

    /// An arrow body means different things depending on whether the member
    /// produces a value. Reading `=> expr` as a discarded expression on an
    /// operator would silently throw its result away.
    #[test]
    fn an_arrow_body_returns_only_where_a_value_is_expected() {
        let module = parse_ok(
            "struct S {
                 static S operator +(S a, S b) => a;
             }
             behavior B {
                 void Tick(float dt) => Move(dt);
                 int Score() => 42;
                 on Lost => become Patrol;
             }",
        );

        let Item::Struct(decl) = &module.items[0] else {
            panic!("expected a struct");
        };
        assert!(
            matches!(decl.operators[0].body.statements[0], Stmt::Return { .. }),
            "an operator's arrow body returns its value"
        );

        let Item::Behavior(behavior) = &module.items[1] else {
            panic!("expected a behavior");
        };

        let BehaviorMember::Method(void_method) = &behavior.members[0] else {
            panic!("expected a method");
        };
        assert!(
            matches!(void_method.body.statements[0], Stmt::Expr(_)),
            "a void method evaluates for effect"
        );

        let BehaviorMember::Method(valued) = &behavior.members[1] else {
            panic!("expected a method");
        };
        assert!(
            matches!(valued.body.statements[0], Stmt::Return { .. }),
            "a non-void method returns"
        );

        let BehaviorMember::Handler(handler) = &behavior.members[2] else {
            panic!("expected a handler");
        };
        assert!(
            matches!(handler.body.statements[0], Stmt::Become { .. }),
            "a statement after the arrow is taken as written"
        );
    }

    #[test]
    fn attributes_attach_to_a_behavior() {
        let module = parse_ok("[Critical] [Budget(0.2ms)] behavior AI { }");
        let Item::Behavior(behavior) = &module.items[0] else {
            panic!("expected a behavior");
        };
        assert_eq!(behavior.attributes.len(), 2);
        assert_eq!(behavior.attributes[0].name, "Critical");
        assert_eq!(behavior.attributes[1].name, "Budget");
        assert_eq!(behavior.attributes[1].args.len(), 1);
    }

    #[test]
    fn imports_parse_with_and_without_an_alias() {
        let module = parse_ok(
            r#"import "combat/damage.erg";
               import "ai/steering.erg" as Steering;"#,
        );
        assert_eq!(module.imports.len(), 2);
        assert_eq!(module.imports[0].path, "combat/damage.erg");
        assert_eq!(module.imports[0].alias, None);
        assert_eq!(module.imports[1].alias.as_deref(), Some("Steering"));
    }

    #[test]
    fn operator_overloads_parse_on_a_struct() {
        let module = parse_ok(
            "struct Health {
                 int current;
                 static Health operator -(Health h, int damage) => h;
                 static bool operator <(Health a, Health b) => true;
             }",
        );
        let Item::Struct(decl) = &module.items[0] else {
            panic!("expected a struct");
        };
        assert_eq!(decl.fields.len(), 1);
        assert_eq!(decl.operators.len(), 2);
        assert_eq!(decl.operators[0].op, OverloadableOp::Sub);
        assert_eq!(decl.operators[1].op, OverloadableOp::Less);
    }

    /// Short-circuiting must not be redefinable, and the message says why
    /// rather than just refusing.
    #[test]
    fn overloading_and_is_refused_with_a_reason() {
        let errors = parse_errors("struct S { static bool operator &&(S a, S b) => true; }");
        assert!(
            errors.iter().any(|e| e.contains("cannot be overloaded")),
            "got {errors:?}"
        );
    }

    #[test]
    fn precedence_follows_c_sharp() {
        let module = parse_ok("fn void F() { var x = 1 + 2 * 3; }");
        let Item::Function(function) = &module.items[0] else {
            panic!("expected a function");
        };
        let Stmt::Let {
            value: Some(expr), ..
        } = &function.body.statements[0]
        else {
            panic!("expected a let");
        };
        // Multiplication binds tighter, so the top node is the addition.
        let Expr::Binary { op, rhs, .. } = expr else {
            panic!("expected a binary expression");
        };
        assert_eq!(*op, BinaryOp::Add);
        assert!(matches!(
            **rhs,
            Expr::Binary {
                op: BinaryOp::Mul,
                ..
            }
        ));
    }

    #[test]
    fn assignment_is_right_associative() {
        let module = parse_ok("fn void F() { a = b = c; }");
        let Item::Function(function) = &module.items[0] else {
            panic!("expected a function");
        };
        let Stmt::Expr(Expr::Assign { value, .. }) = &function.body.statements[0] else {
            panic!("expected an assignment");
        };
        assert!(matches!(**value, Expr::Assign { .. }));
    }

    #[test]
    fn compound_assignment_keeps_its_operator() {
        let module = parse_ok("fn void F() { health -= amount; }");
        let Item::Function(function) = &module.items[0] else {
            panic!("expected a function");
        };
        let Stmt::Expr(Expr::Assign { op, .. }) = &function.body.statements[0] else {
            panic!("expected an assignment");
        };
        assert_eq!(*op, Some(BinaryOp::Sub));
    }

    #[test]
    fn optional_and_array_types_stack() {
        let module = parse_ok("behavior B { Entity? target; Vec3[] path; Entity[]? maybe; }");
        let Item::Behavior(behavior) = &module.items[0] else {
            panic!("expected a behavior");
        };
        let types: Vec<_> = behavior
            .members
            .iter()
            .filter_map(|m| match m {
                BehaviorMember::Field(field) => Some(&field.ty),
                _ => None,
            })
            .collect();

        assert!(matches!(types[0], TypeRef::Optional { .. }));
        assert!(matches!(types[1], TypeRef::Array { .. }));
        let TypeRef::Optional { inner, .. } = types[2] else {
            panic!("expected an optional");
        };
        assert!(
            matches!(**inner, TypeRef::Array { .. }),
            "an optional array"
        );
    }

    /// A field and a method both open with `Type Name`; only the `(` separates
    /// them, and the lookahead has to see past `[]`, `?` and `<…>`.
    #[test]
    fn fields_and_methods_are_told_apart() {
        let module = parse_ok(
            "behavior B {
                 Map<string, int> table;
                 Vec3[] Path(int n) { return null; }
                 Entity? target;
                 Entity? Find(string name) { return null; }
             }",
        );
        let Item::Behavior(behavior) = &module.items[0] else {
            panic!("expected a behavior");
        };
        let kinds: Vec<_> = behavior
            .members
            .iter()
            .map(|m| matches!(m, BehaviorMember::Method(_)))
            .collect();
        assert_eq!(kinds, vec![false, true, false, true]);
    }

    #[test]
    fn await_parses_as_a_prefix_expression() {
        let module = parse_ok("behavior B { async void Attack() { await 0.5s; } }");
        let Item::Behavior(behavior) = &module.items[0] else {
            panic!("expected a behavior");
        };
        let BehaviorMember::Method(method) = &behavior.members[0] else {
            panic!("expected a method");
        };
        assert!(method.is_async);
        assert!(matches!(
            method.body.statements[0],
            Stmt::Expr(Expr::Await { .. })
        ));
    }

    /// The parser accepts `await` anywhere; placing it correctly is the type
    /// checker's job, which can name the enclosing member in its message.
    #[test]
    fn await_outside_async_is_left_to_the_type_checker() {
        let module = parse_ok("behavior B { void Update(float dt) { await 1s; } }");
        assert_eq!(module.items.len(), 1, "syntax alone is well-formed");
    }

    #[test]
    fn match_arms_parse_with_bindings_and_null() {
        let module = parse_ok(
            "fn void F() {
                 match (target) {
                     Entity e => Attack(e),
                     null => Idle(),
                 }
             }",
        );
        let Item::Function(function) = &module.items[0] else {
            panic!("expected a function");
        };
        let Stmt::Match { arms, .. } = &function.body.statements[0] else {
            panic!("expected a match");
        };
        assert_eq!(arms.len(), 2);
        assert!(matches!(arms[0].pattern, Pattern::Binding { .. }));
        assert!(matches!(arms[1].pattern, Pattern::Null(_)));
    }

    #[test]
    fn loops_and_branches_parse() {
        parse_ok(
            "fn void F() {
                 for (int i = 0; i < 10; i = i + 1) { }
                 foreach (var x in items) { }
                 while (running) { break; }
                 if (a) { } else if (b) { } else { }
             }",
        );
    }

    #[test]
    fn casts_are_distinguished_from_grouping() {
        let module = parse_ok("fn void F() { var a = (float)n; var b = (n + 1) * 2; }");
        let Item::Function(function) = &module.items[0] else {
            panic!("expected a function");
        };
        let Stmt::Let {
            value: Some(cast), ..
        } = &function.body.statements[0]
        else {
            panic!("expected a let");
        };
        assert!(matches!(cast, Expr::Cast { .. }));

        let Stmt::Let {
            value: Some(group), ..
        } = &function.body.statements[1]
        else {
            panic!("expected a let");
        };
        assert!(matches!(
            group,
            Expr::Binary {
                op: BinaryOp::Mul,
                ..
            }
        ));
    }

    #[test]
    fn optional_access_and_coalesce_parse() {
        let module = parse_ok("fn void F() { var name = target?.name ?? \"none\"; }");
        let Item::Function(function) = &module.items[0] else {
            panic!("expected a function");
        };
        let Stmt::Let {
            value: Some(expr), ..
        } = &function.body.statements[0]
        else {
            panic!("expected a let");
        };
        let Expr::Binary { op, lhs, .. } = expr else {
            panic!("expected a binary expression");
        };
        assert_eq!(*op, BinaryOp::Coalesce);
        assert!(matches!(**lhs, Expr::OptionalField { .. }));
    }

    /// `var` without an initialiser has nothing to infer from, and the message
    /// says so rather than reporting a missing token.
    #[test]
    fn var_without_an_initialiser_explains_itself() {
        let lexed = lex("fn void F() { var x; }");
        let parsed = parse(lexed.tokens);
        let note = parsed
            .diagnostics
            .iter()
            .find_map(|d| d.note.as_deref())
            .unwrap_or_default();
        assert!(note.contains("infers the type"), "got note: {note}");
    }

    /// One run reports several problems: the author should not have to
    /// recompile once per mistake.
    #[test]
    fn parsing_recovers_and_reports_more_than_one_error() {
        let errors = parse_errors(
            "fn void A() { var x = ; }
             fn void B() { var y = ; }",
        );
        assert!(errors.len() >= 2, "got {errors:?}");
    }

    /// Recovery must reach the next declaration, so a broken one does not
    /// swallow the rest of the file.
    #[test]
    fn a_broken_declaration_does_not_hide_the_next() {
        let lexed = lex("behavior { } behavior Good { }");
        let parsed = parse(lexed.tokens);
        assert!(parsed.has_errors());
        assert!(
            parsed.module.items.iter().any(|item| item.name() == "Good"),
            "the second behavior should still be parsed"
        );
    }

    #[test]
    fn attributes_on_a_struct_are_refused_with_a_reason() {
        let lexed = lex("[Critical] struct S { }");
        let parsed = parse(lexed.tokens);
        let note = parsed
            .diagnostics
            .iter()
            .find_map(|d| d.note.as_deref())
            .unwrap_or_default();
        assert!(note.contains("behaviors"), "got note: {note}");
    }

    #[test]
    fn an_empty_file_parses_to_an_empty_module() {
        let module = parse_ok("");
        assert!(module.items.is_empty());
        assert!(module.imports.is_empty());
    }
}

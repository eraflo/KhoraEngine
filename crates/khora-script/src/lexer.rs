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

use crate::diagnostics::{Diagnostic, Span};

/// A lexed token: what it is, and where it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    /// The token itself.
    pub kind: TokenKind,
    /// Its position in the source.
    pub span: Span,
}

/// Every token Ergon recognises.
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // ── Literals ──────────────────────────────────────
    /// Whole number: `42`, `0x2A`, `1_000`.
    Int(i64),
    /// Fractional number: `2.0`, `1e-3`. No `f` suffix.
    Float(f32),
    /// `"text"`.
    Str(String),
    /// A duration, **normalised to seconds**: `2s`, `500ms`, `1.5min`.
    ///
    /// Normalising at lex time means the rest of the compiler never has to know
    /// which spelling was used.
    Duration(f32),
    /// An angle, **normalised to radians**: `90deg`, `1.57rad`.
    Angle(f32),

    // ── Names ─────────────────────────────────────────
    /// An identifier that is not a keyword.
    Ident(String),
    /// A reserved word.
    Keyword(Keyword),

    // ── Punctuation and operators ─────────────────────
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `{`
    LBrace,
    /// `}`
    RBrace,
    /// `[`
    LBracket,
    /// `]`
    RBracket,
    /// `,`
    Comma,
    /// `;`
    Semi,
    /// `.`
    Dot,
    /// `:`
    Colon,
    /// `=`
    Assign,
    /// `=>` — arrow-bodied members and transitions.
    FatArrow,
    /// `+`
    Plus,
    /// `-`
    Minus,
    /// `*`
    Star,
    /// `/`
    Slash,
    /// `%`
    Percent,
    /// `+=`
    PlusAssign,
    /// `-=`
    MinusAssign,
    /// `*=`
    StarAssign,
    /// `/=`
    SlashAssign,
    /// `==`
    Eq,
    /// `!=`
    NotEq,
    /// `<`
    Less,
    /// `<=`
    LessEq,
    /// `>`
    Greater,
    /// `>=`
    GreaterEq,
    /// `&&`
    AndAnd,
    /// `||`
    OrOr,
    /// `!`
    Bang,
    /// `?` — introduces an optional type, and the ternary.
    Question,
    /// `??`
    QuestionQuestion,
    /// `?.`
    QuestionDot,
    /// `@` — attribute marker in `[Critical]`-style positions is `[`, but `@`
    /// stays reserved so it cannot be used as an identifier later.
    At,

    /// End of input. Emitted once, so the parser can report "unexpected end"
    /// with a real span instead of an absence.
    Eof,
}

/// Reserved words.
///
/// `int`, `float`, `bool` and `string` are **not** here: they are ordinary
/// identifiers resolved by the type checker. That keeps `Vec3` and `Health` on
/// exactly the same footing as the built-ins, so a user type is never a
/// second-class citizen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keyword {
    /// `behavior`
    Behavior,
    /// `state`
    State,
    /// `become`
    Become,
    /// `on`
    On,
    /// `every`
    Every,
    /// `after`
    After,
    /// `async`
    Async,
    /// `await`
    Await,
    /// `import`
    Import,
    /// `as`
    As,
    /// `struct`
    Struct,
    /// `fn`
    Fn,
    /// `var`
    Var,
    /// `void`
    Void,
    /// `static`
    Static,
    /// `operator`
    Operator,
    /// `new`
    New,
    /// `if`
    If,
    /// `else`
    Else,
    /// `while`
    While,
    /// `for`
    For,
    /// `foreach`
    Foreach,
    /// `in`
    In,
    /// `match`
    Match,
    /// `return`
    Return,
    /// `break`
    Break,
    /// `continue`
    Continue,
    /// `true`
    True,
    /// `false`
    False,
    /// `null`
    Null,
    /// `this`
    This,
}

impl Keyword {
    /// The keyword `word` spells, if it is one.
    fn from_word(word: &str) -> Option<Self> {
        Some(match word {
            "behavior" => Self::Behavior,
            "state" => Self::State,
            "become" => Self::Become,
            "on" => Self::On,
            "every" => Self::Every,
            "after" => Self::After,
            "async" => Self::Async,
            "await" => Self::Await,
            "import" => Self::Import,
            "as" => Self::As,
            "struct" => Self::Struct,
            "fn" => Self::Fn,
            "var" => Self::Var,
            "void" => Self::Void,
            "static" => Self::Static,
            "operator" => Self::Operator,
            "new" => Self::New,
            "if" => Self::If,
            "else" => Self::Else,
            "while" => Self::While,
            "for" => Self::For,
            "foreach" => Self::Foreach,
            "in" => Self::In,
            "match" => Self::Match,
            "return" => Self::Return,
            "break" => Self::Break,
            "continue" => Self::Continue,
            "true" => Self::True,
            "false" => Self::False,
            "null" => Self::Null,
            "this" => Self::This,
            _ => return None,
        })
    }
}

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

struct Lexer<'a> {
    source: &'a str,
    bytes: &'a [u8],
    offset: usize,
    tokens: Vec<Token>,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            offset: 0,
            tokens: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn run(mut self) -> Lexed {
        loop {
            self.skip_trivia();
            let start = self.offset;
            let Some(byte) = self.peek() else {
                break;
            };

            let kind = if byte.is_ascii_digit() {
                self.lex_number(start)
            } else if byte == b'"' {
                self.lex_string(start)
            } else if is_ident_start(byte) {
                Some(self.lex_word(start))
            } else {
                self.lex_symbol(start)
            };

            if let Some(kind) = kind {
                let span = Span::new(start as u32, self.offset as u32);
                self.tokens.push(Token { kind, span });
            }
        }

        let end = self.source.len() as u32;
        self.tokens.push(Token {
            kind: TokenKind::Eof,
            span: Span::empty(end),
        });

        Lexed {
            tokens: self.tokens,
            diagnostics: self.diagnostics,
        }
    }

    // ── Cursor ────────────────────────────────────────

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.offset).copied()
    }

    fn peek_at(&self, ahead: usize) -> Option<u8> {
        self.bytes.get(self.offset + ahead).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        // Advance by the whole UTF-8 sequence, so a multi-byte character inside
        // a string or comment cannot leave the cursor mid-character.
        self.offset += utf8_len(byte);
        Some(byte)
    }

    /// Consumes `expected` if it is next.
    fn eat(&mut self, expected: u8) -> bool {
        if self.peek() == Some(expected) {
            self.offset += 1;
            true
        } else {
            false
        }
    }

    /// Skips whitespace and comments, however many of each follow.
    fn skip_trivia(&mut self) {
        loop {
            match self.peek() {
                Some(b) if b.is_ascii_whitespace() => {
                    self.offset += 1;
                }
                Some(b'/') if self.peek_at(1) == Some(b'/') => {
                    while let Some(b) = self.peek() {
                        if b == b'\n' {
                            break;
                        }
                        self.bump();
                    }
                }
                Some(b'/') if self.peek_at(1) == Some(b'*') => self.skip_block_comment(),
                _ => return,
            }
        }
    }

    /// Skips `/* … */`, nesting included — a commented-out region containing a
    /// comment should not end early on the inner terminator.
    fn skip_block_comment(&mut self) {
        let start = self.offset;
        self.offset += 2;
        let mut depth = 1usize;

        while depth > 0 {
            match (self.peek(), self.peek_at(1)) {
                (Some(b'/'), Some(b'*')) => {
                    self.offset += 2;
                    depth += 1;
                }
                (Some(b'*'), Some(b'/')) => {
                    self.offset += 2;
                    depth -= 1;
                }
                (Some(_), _) => {
                    self.bump();
                }
                (None, _) => {
                    self.diagnostics.push(
                        Diagnostic::error(
                            "unterminated block comment",
                            Span::new(start as u32, self.offset as u32),
                        )
                        .with_note("add `*/` to close it"),
                    );
                    return;
                }
            }
        }
    }

    // ── Token kinds ───────────────────────────────────

    /// Numbers, and the units that may follow them.
    fn lex_number(&mut self, start: usize) -> Option<TokenKind> {
        if self.peek() == Some(b'0') && matches!(self.peek_at(1), Some(b'x') | Some(b'X')) {
            return self.lex_hex(start);
        }

        let mut seen_dot = false;
        let mut seen_exponent = false;

        while let Some(byte) = self.peek() {
            match byte {
                b'0'..=b'9' | b'_' => {
                    self.offset += 1;
                }
                // A dot is part of the number only if a digit follows, so
                // `2.Method()` still lexes as `2` `.` `Method`.
                b'.' if !seen_dot
                    && !seen_exponent
                    && self.peek_at(1).is_some_and(|b| b.is_ascii_digit()) =>
                {
                    seen_dot = true;
                    self.offset += 1;
                }
                b'e' | b'E' if !seen_exponent && self.exponent_follows() => {
                    seen_exponent = true;
                    self.offset += 1;
                    // Consume the optional sign; digits are handled by the loop.
                    if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                        self.offset += 1;
                    }
                }
                _ => break,
            }
        }

        let digits: String = self.source[start..self.offset]
            .chars()
            .filter(|c| *c != '_')
            .collect();
        let unit = self.lex_unit_suffix();

        match unit {
            Some(Unit::Duration(scale)) => match digits.parse::<f32>() {
                Ok(value) => Some(TokenKind::Duration(value * scale)),
                Err(_) => self.number_error(start, &digits),
            },
            Some(Unit::Angle(scale)) => match digits.parse::<f32>() {
                Ok(value) => Some(TokenKind::Angle(value * scale)),
                Err(_) => self.number_error(start, &digits),
            },
            None if seen_dot || seen_exponent => match digits.parse::<f32>() {
                Ok(value) => Some(TokenKind::Float(value)),
                Err(_) => self.number_error(start, &digits),
            },
            None => match digits.parse::<i64>() {
                Ok(value) => Some(TokenKind::Int(value)),
                Err(_) => {
                    self.diagnostics.push(
                        Diagnostic::error(
                            format!("integer literal `{digits}` does not fit in an int"),
                            Span::new(start as u32, self.offset as u32),
                        )
                        .with_note("int is 64-bit signed"),
                    );
                    None
                }
            },
        }
    }

    /// Whether an `e` at the cursor really starts an exponent.
    ///
    /// Without this, `2end` would lex as a malformed float rather than `2`
    /// followed by the identifier `end`.
    fn exponent_follows(&self) -> bool {
        match self.peek_at(1) {
            Some(b) if b.is_ascii_digit() => true,
            Some(b'+') | Some(b'-') => self.peek_at(2).is_some_and(|b| b.is_ascii_digit()),
            _ => false,
        }
    }

    fn lex_hex(&mut self, start: usize) -> Option<TokenKind> {
        self.offset += 2;
        let digits_start = self.offset;
        while self
            .peek()
            .is_some_and(|b| b.is_ascii_hexdigit() || b == b'_')
        {
            self.offset += 1;
        }

        let digits: String = self.source[digits_start..self.offset]
            .chars()
            .filter(|c| *c != '_')
            .collect();

        if digits.is_empty() {
            self.diagnostics.push(
                Diagnostic::error(
                    "hex literal has no digits",
                    Span::new(start as u32, self.offset as u32),
                )
                .with_note("write `0x` followed by at least one hex digit"),
            );
            return None;
        }

        match i64::from_str_radix(&digits, 16) {
            Ok(value) => Some(TokenKind::Int(value)),
            Err(_) => {
                self.diagnostics.push(
                    Diagnostic::error(
                        format!("hex literal `0x{digits}` does not fit in an int"),
                        Span::new(start as u32, self.offset as u32),
                    )
                    .with_note("int is 64-bit signed"),
                );
                None
            }
        }
    }

    /// Reads a unit suffix, if the identifier right after the digits is one.
    ///
    /// A suffix must touch the number: `2s` is a duration, `2 s` is `2` and the
    /// variable `s`. Anything else would make whitespace meaningful in a way
    /// nothing else in the language is.
    fn lex_unit_suffix(&mut self) -> Option<Unit> {
        let start = self.offset;
        if !self.peek().is_some_and(is_ident_start) {
            return None;
        }
        let mut end = start;
        while self.bytes.get(end).copied().is_some_and(is_ident_continue) {
            end += 1;
        }

        let unit = match &self.source[start..end] {
            "s" => Unit::Duration(1.0),
            "ms" => Unit::Duration(0.001),
            "min" => Unit::Duration(60.0),
            "deg" => Unit::Angle(std::f32::consts::PI / 180.0),
            "rad" => Unit::Angle(1.0),
            // Not a unit: leave the cursor alone so it lexes as an identifier.
            // `2x` therefore becomes `2` then `x`, which the parser rejects
            // with a better message than the lexer could give here.
            _ => return None,
        };
        self.offset = end;
        Some(unit)
    }

    fn number_error(&mut self, start: usize, digits: &str) -> Option<TokenKind> {
        self.diagnostics.push(Diagnostic::error(
            format!("`{digits}` is not a valid number"),
            Span::new(start as u32, self.offset as u32),
        ));
        None
    }

    fn lex_string(&mut self, start: usize) -> Option<TokenKind> {
        self.offset += 1;
        let mut text = String::new();

        loop {
            let Some(byte) = self.peek() else {
                self.diagnostics.push(
                    Diagnostic::error(
                        "unterminated string",
                        Span::new(start as u32, self.offset as u32),
                    )
                    .with_note("add a closing `\"`"),
                );
                return None;
            };

            match byte {
                b'"' => {
                    self.offset += 1;
                    return Some(TokenKind::Str(text));
                }
                // A string may not span lines: an unclosed quote would
                // otherwise swallow the rest of the file and report the error
                // far from its cause.
                b'\n' => {
                    self.diagnostics.push(
                        Diagnostic::error(
                            "unterminated string",
                            Span::new(start as u32, self.offset as u32),
                        )
                        .with_note("strings cannot span lines"),
                    );
                    return None;
                }
                b'\\' => {
                    self.offset += 1;
                    let escape = self.bump();
                    match escape {
                        Some(b'n') => text.push('\n'),
                        Some(b't') => text.push('\t'),
                        Some(b'r') => text.push('\r'),
                        Some(b'\\') => text.push('\\'),
                        Some(b'"') => text.push('"'),
                        Some(other) => {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    format!("unknown escape `\\{}`", other as char),
                                    Span::new(self.offset as u32 - 2, self.offset as u32),
                                )
                                .with_note("known escapes are \\n \\t \\r \\\\ \\\""),
                            );
                        }
                        None => {}
                    }
                }
                _ => {
                    let from = self.offset;
                    self.bump();
                    text.push_str(&self.source[from..self.offset]);
                }
            }
        }
    }

    fn lex_word(&mut self, start: usize) -> TokenKind {
        while self.peek().is_some_and(is_ident_continue) {
            self.offset += 1;
        }
        let word = &self.source[start..self.offset];
        match Keyword::from_word(word) {
            Some(keyword) => TokenKind::Keyword(keyword),
            None => TokenKind::Ident(word.to_owned()),
        }
    }

    fn lex_symbol(&mut self, start: usize) -> Option<TokenKind> {
        let byte = self.bump()?;
        let kind = match byte {
            b'(' => TokenKind::LParen,
            b')' => TokenKind::RParen,
            b'{' => TokenKind::LBrace,
            b'}' => TokenKind::RBrace,
            b'[' => TokenKind::LBracket,
            b']' => TokenKind::RBracket,
            b',' => TokenKind::Comma,
            b';' => TokenKind::Semi,
            b'.' => TokenKind::Dot,
            b':' => TokenKind::Colon,
            b'@' => TokenKind::At,
            b'=' if self.eat(b'=') => TokenKind::Eq,
            b'=' if self.eat(b'>') => TokenKind::FatArrow,
            b'=' => TokenKind::Assign,
            b'+' if self.eat(b'=') => TokenKind::PlusAssign,
            b'+' => TokenKind::Plus,
            b'-' if self.eat(b'=') => TokenKind::MinusAssign,
            b'-' => TokenKind::Minus,
            b'*' if self.eat(b'=') => TokenKind::StarAssign,
            b'*' => TokenKind::Star,
            b'/' if self.eat(b'=') => TokenKind::SlashAssign,
            b'/' => TokenKind::Slash,
            b'%' => TokenKind::Percent,
            b'!' if self.eat(b'=') => TokenKind::NotEq,
            b'!' => TokenKind::Bang,
            b'<' if self.eat(b'=') => TokenKind::LessEq,
            b'<' => TokenKind::Less,
            b'>' if self.eat(b'=') => TokenKind::GreaterEq,
            b'>' => TokenKind::Greater,
            b'&' if self.eat(b'&') => TokenKind::AndAnd,
            b'|' if self.eat(b'|') => TokenKind::OrOr,
            b'?' if self.eat(b'?') => TokenKind::QuestionQuestion,
            b'?' if self.eat(b'.') => TokenKind::QuestionDot,
            b'?' => TokenKind::Question,
            b'&' => {
                self.unknown(
                    start,
                    "`&`",
                    "did you mean `&&`? Ergon has no bitwise operators",
                );
                return None;
            }
            b'|' => {
                self.unknown(
                    start,
                    "`|`",
                    "did you mean `||`? Ergon has no bitwise operators",
                );
                return None;
            }
            _ => {
                let text = &self.source[start..self.offset];
                self.diagnostics.push(Diagnostic::error(
                    format!("unexpected character `{text}`"),
                    Span::new(start as u32, self.offset as u32),
                ));
                return None;
            }
        };
        Some(kind)
    }

    fn unknown(&mut self, start: usize, what: &str, note: &str) {
        self.diagnostics.push(
            Diagnostic::error(
                format!("unexpected character {what}"),
                Span::new(start as u32, self.offset as u32),
            )
            .with_note(note),
        );
    }
}

/// A recognised unit suffix and the factor that normalises it.
enum Unit {
    /// To seconds.
    Duration(f32),
    /// To radians.
    Angle(f32),
}

fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_ident_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Length in bytes of the UTF-8 sequence starting with `first`.
fn utf8_len(first: u8) -> usize {
    match first {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        // A continuation byte here means the cursor is already desynchronised;
        // advancing by one resynchronises rather than looping forever.
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The token kinds, with `Eof` dropped — every test would repeat it.
    fn kinds(source: &str) -> Vec<TokenKind> {
        let lexed = lex(source);
        assert!(
            !lexed.has_errors(),
            "expected a clean lex, got {:?}",
            lexed.diagnostics
        );
        lexed
            .tokens
            .into_iter()
            .map(|t| t.kind)
            .filter(|k| *k != TokenKind::Eof)
            .collect()
    }

    fn errors(source: &str) -> Vec<String> {
        lex(source)
            .diagnostics
            .into_iter()
            .map(|d| d.message)
            .collect()
    }

    #[test]
    fn integers_floats_and_separators() {
        assert_eq!(kinds("42"), vec![TokenKind::Int(42)]);
        assert_eq!(kinds("1_000_000"), vec![TokenKind::Int(1_000_000)]);
        assert_eq!(kinds("0x2A"), vec![TokenKind::Int(42)]);
        assert_eq!(kinds("2.5"), vec![TokenKind::Float(2.5)]);
        assert_eq!(kinds("1e-3"), vec![TokenKind::Float(0.001)]);
    }

    /// No `f` suffix: gameplay is mostly floats, and the noise buys nothing.
    #[test]
    fn a_float_needs_no_suffix() {
        assert_eq!(kinds("2.0"), vec![TokenKind::Float(2.0)]);
        // `2.0f` is `2.0` then the identifier `f` — the parser will complain,
        // with a better message than the lexer could produce.
        assert_eq!(
            kinds("2.0f"),
            vec![TokenKind::Float(2.0), TokenKind::Ident("f".into())]
        );
    }

    /// Durations normalise to seconds at lex time, so nothing downstream has to
    /// know which spelling was used.
    #[test]
    fn durations_normalise_to_seconds() {
        assert_eq!(kinds("2s"), vec![TokenKind::Duration(2.0)]);
        assert_eq!(kinds("500ms"), vec![TokenKind::Duration(0.5)]);
        assert_eq!(kinds("1.5min"), vec![TokenKind::Duration(90.0)]);
    }

    #[test]
    fn angles_normalise_to_radians() {
        let TokenKind::Angle(radians) = kinds("90deg")[0].clone() else {
            panic!("expected an angle");
        };
        assert!((radians - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
        assert_eq!(kinds("1.0rad"), vec![TokenKind::Angle(1.0)]);
    }

    /// A unit has to touch its number. Otherwise whitespace would carry meaning
    /// nowhere else in the language gives it.
    #[test]
    fn a_detached_suffix_is_an_identifier() {
        assert_eq!(
            kinds("2 s"),
            vec![TokenKind::Int(2), TokenKind::Ident("s".into())]
        );
    }

    /// An unknown suffix is not swallowed: `2x` is `2` then `x`, so the parser
    /// gets to explain the problem in context.
    #[test]
    fn an_unknown_suffix_stays_an_identifier() {
        assert_eq!(
            kinds("2x"),
            vec![TokenKind::Int(2), TokenKind::Ident("x".into())]
        );
    }

    /// `2end` must not be read as a malformed exponent.
    #[test]
    fn an_e_that_is_not_an_exponent() {
        assert_eq!(
            kinds("2end"),
            vec![TokenKind::Int(2), TokenKind::Ident("end".into())]
        );
    }

    /// A dot only joins the number when a digit follows, so method calls on a
    /// literal still lex.
    #[test]
    fn a_trailing_dot_is_not_part_of_the_number() {
        assert_eq!(
            kinds("2.Method"),
            vec![
                TokenKind::Int(2),
                TokenKind::Dot,
                TokenKind::Ident("Method".into())
            ]
        );
    }

    #[test]
    fn keywords_are_distinguished_from_identifiers() {
        assert_eq!(
            kinds("behavior Guard state become"),
            vec![
                TokenKind::Keyword(Keyword::Behavior),
                TokenKind::Ident("Guard".into()),
                TokenKind::Keyword(Keyword::State),
                TokenKind::Keyword(Keyword::Become),
            ]
        );
    }

    /// Type names are ordinary identifiers, so a user type is never a
    /// second-class citizen next to the built-ins.
    #[test]
    fn type_names_are_not_keywords() {
        assert_eq!(
            kinds("int float Health"),
            vec![
                TokenKind::Ident("int".into()),
                TokenKind::Ident("float".into()),
                TokenKind::Ident("Health".into()),
            ]
        );
    }

    #[test]
    fn multi_character_operators_win_over_single() {
        assert_eq!(
            kinds("== != <= >= && || += -= *= /= => ?? ?."),
            vec![
                TokenKind::Eq,
                TokenKind::NotEq,
                TokenKind::LessEq,
                TokenKind::GreaterEq,
                TokenKind::AndAnd,
                TokenKind::OrOr,
                TokenKind::PlusAssign,
                TokenKind::MinusAssign,
                TokenKind::StarAssign,
                TokenKind::SlashAssign,
                TokenKind::FatArrow,
                TokenKind::QuestionQuestion,
                TokenKind::QuestionDot,
            ]
        );
    }

    #[test]
    fn strings_handle_escapes() {
        assert_eq!(
            kinds(r#""a\nb\"c""#),
            vec![TokenKind::Str("a\nb\"c".into())]
        );
    }

    /// Non-ASCII inside a string must survive intact — the cursor advances by
    /// whole UTF-8 sequences.
    #[test]
    fn strings_carry_non_ascii_through() {
        assert_eq!(
            kinds(r#""héllo → ok""#),
            vec![TokenKind::Str("héllo → ok".into())]
        );
    }

    #[test]
    fn comments_are_skipped_including_nested_blocks() {
        assert_eq!(
            kinds("1 // trailing\n/* outer /* inner */ still */ 2"),
            vec![TokenKind::Int(1), TokenKind::Int(2)]
        );
    }

    #[test]
    fn an_unterminated_string_is_reported_not_swallowed() {
        let messages = errors("\"open\nvar x = 1;");
        assert_eq!(messages, vec!["unterminated string"]);
    }

    #[test]
    fn an_unterminated_block_comment_is_reported() {
        assert_eq!(errors("/* forever"), vec!["unterminated block comment"]);
    }

    /// The lexer keeps going after a bad character, so one run reports every
    /// lexical problem rather than making the author iterate.
    #[test]
    fn lexing_continues_after_an_error() {
        let lexed = lex("var x = #;\nvar y = $;");
        assert_eq!(lexed.diagnostics.len(), 2, "both bad characters reported");
        assert!(
            lexed
                .tokens
                .iter()
                .any(|t| t.kind == TokenKind::Ident("y".into())),
            "tokens after the first error are still produced"
        );
    }

    /// Bitwise operators do not exist; saying so is more useful than
    /// "unexpected character".
    #[test]
    fn a_single_ampersand_explains_itself() {
        let diagnostics = lex("a & b").diagnostics;
        assert_eq!(diagnostics.len(), 1);
        let note = diagnostics[0].note.as_deref().unwrap_or_default();
        assert!(note.contains("&&"), "got note: {note}");
    }

    #[test]
    fn eof_is_emitted_with_a_span() {
        let lexed = lex("42");
        let last = lexed.tokens.last().expect("at least Eof");
        assert_eq!(last.kind, TokenKind::Eof);
        assert_eq!(last.span, Span::empty(2));
    }

    #[test]
    fn spans_point_at_the_token() {
        let lexed = lex("  behavior");
        assert_eq!(lexed.tokens[0].span, Span::new(2, 10));
    }
}

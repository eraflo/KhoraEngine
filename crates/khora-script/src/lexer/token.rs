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

//! The token vocabulary.
//!
//! Separate from the scanner because it is the shared contract: the parser
//! matches on these and never touches the scanning logic.

use crate::diagnostics::Span;

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
    pub(super) fn from_word(word: &str) -> Option<Self> {
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

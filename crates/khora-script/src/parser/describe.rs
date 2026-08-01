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

//! Human names for tokens, used by "expected X, found Y".
//!
//! Kept apart from the parsing logic: it is a lookup table that grows with the
//! token set, and mixing it into the grammar would bury the grammar.

use crate::lexer::{Keyword, TokenKind};

/// A human name for a token, for "expected X, found Y".
pub(super) fn describe(kind: &TokenKind) -> String {
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

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

//! `$"hp: {h.current} / {h.max}"` — text and holes.
//!
//! A hole runs to the `}` that closes its `{`: braces are counted, and a
//! string inside it — an interpolated one too — is skipped whole, so a `}` in
//! `$"{F("}")}"` belongs to the inner text. The hole's text is lexed on its
//! own and its tokens placed where it sits, so every message about it points
//! into the file. `{{` and `}}` are a literal brace.

use super::scanner::Lexer;
use super::token::{Piece, TokenKind};
use crate::diagnostics::{Diagnostic, Span};

impl Lexer<'_> {
    /// `$"…"`, its `$` at `start` and its `"` next.
    pub(super) fn lex_interpolated(&mut self, start: usize) -> Option<TokenKind> {
        self.offset += 2;
        let mut pieces: Vec<Piece> = Vec::new();
        let mut text = String::new();
        loop {
            let Some(byte) = self.peek() else {
                self.unterminated(start, "add a closing `\"`");
                return None;
            };
            match byte {
                b'"' => {
                    self.offset += 1;
                    if !text.is_empty() {
                        pieces.push(Piece::Text(text));
                    }
                    return Some(TokenKind::Interpolated(pieces));
                }
                b'\n' => {
                    self.unterminated(start, "strings cannot span lines");
                    return None;
                }
                b'\\' => self.escape_into(&mut text),
                b'{' if self.bytes.get(self.offset + 1).copied() == Some(b'{') => {
                    self.offset += 2;
                    text.push('{');
                }
                b'}' if self.bytes.get(self.offset + 1).copied() == Some(b'}') => {
                    self.offset += 2;
                    text.push('}');
                }
                b'}' => {
                    self.diagnostics.push(
                        Diagnostic::error(
                            "a `}` in an interpolated string is written `}}`",
                            Span::new(self.offset as u32, self.offset as u32 + 1),
                        )
                        .with_note("a lone `}` closes nothing: double it for the brace itself"),
                    );
                    self.offset += 1;
                }
                b'{' => {
                    let open = self.offset;
                    let Some(close) = self.hole_end(open + 1) else {
                        // Its line belongs to the string: lexing it again as
                        // code would report the one mistake twice.
                        self.offset = self.bytes[open..]
                            .iter()
                            .position(|&byte| byte == b'\n')
                            .map_or(self.bytes.len(), |at| open + at);
                        self.unterminated(
                            start,
                            "close the hole with `}`, then the text with `\"`",
                        );
                        return None;
                    };
                    let inner = &self.source[open + 1..close];
                    if inner.trim().is_empty() {
                        self.diagnostics.push(
                            Diagnostic::error(
                                "an empty `{}` in an interpolated string shows nothing",
                                Span::new(open as u32, close as u32 + 1),
                            )
                            .with_note(
                                "put the value to show inside, or write `{{}}` for the braces",
                            ),
                        );
                    } else {
                        let lexed = super::lex(inner).shifted(open as u32 + 1);
                        self.diagnostics.extend(lexed.diagnostics);
                        if !text.is_empty() {
                            pieces.push(Piece::Text(std::mem::take(&mut text)));
                        }
                        pieces.push(Piece::Hole {
                            tokens: lexed.tokens,
                            span: Span::new(open as u32, close as u32 + 1),
                        });
                    }
                    self.offset = close + 1;
                }
                _ => {
                    let from = self.offset;
                    self.bump();
                    text.push_str(&self.source[from..self.offset]);
                }
            }
        }
    }

    /// Where the hole opened just before `from` closes: the `}` matching its
    /// `{`, past any string inside it — `None` when the line ends first.
    fn hole_end(&self, from: usize) -> Option<usize> {
        let bytes = self.bytes;
        let mut depth = 0usize;
        let mut at = from;
        while let Some(&byte) = bytes.get(at) {
            match byte {
                b'\n' => return None,
                b'{' => depth += 1,
                b'}' if depth == 0 => return Some(at),
                b'}' => depth -= 1,
                b'"' => at = skip_string(bytes, at + 1, false)?,
                b'$' if bytes.get(at + 1) == Some(&b'"') => at = skip_string(bytes, at + 2, true)?,
                _ => {}
            }
            at += 1;
        }
        None
    }

    fn unterminated(&mut self, start: usize, note: &str) {
        let span = Span::new(start as u32, self.offset as u32);
        self.diagnostics
            .push(Diagnostic::error("unterminated string", span).with_note(note.to_owned()));
    }
}

/// Skips a string's text from `at`, just past its opening quote, to its
/// closing quote — counting an interpolated one's holes — and returns where
/// that quote sits.
fn skip_string(bytes: &[u8], mut at: usize, interpolated: bool) -> Option<usize> {
    while let Some(&byte) = bytes.get(at) {
        match byte {
            b'\n' => return None,
            b'\\' => at += 1,
            b'"' => return Some(at),
            b'{' if interpolated && bytes.get(at + 1) == Some(&b'{') => at += 1,
            b'{' if interpolated => {
                // The hole's own closing brace, past what it holds.
                let mut depth = 0usize;
                at += 1;
                loop {
                    match *bytes.get(at)? {
                        b'\n' => return None,
                        b'{' => depth += 1,
                        b'}' if depth == 0 => break,
                        b'}' => depth -= 1,
                        b'"' => at = skip_string(bytes, at + 1, false)?,
                        b'$' if bytes.get(at + 1) == Some(&b'"') => {
                            at = skip_string(bytes, at + 2, true)?;
                        }
                        _ => {}
                    }
                    at += 1;
                }
            }
            _ => {}
        }
        at += 1;
    }
    None
}

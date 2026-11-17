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

//! Reads the top-level items of a Rust file, for the in-file order checks.

use super::code::{
    blank_literals, closing_bracket, is_ident_char, leading_ident, strip_rust_comments,
};
use super::modules::is_cfg_test;

/// What a top-level item is, as far as the in-file order checks need.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    /// `use`, and `extern crate`.
    Use,
    /// `const`.
    Const,
    /// `static`.
    Static,
    /// `type Name = …;`
    TypeAlias,
    /// `struct`.
    Struct,
    /// `enum`.
    Enum,
    /// `union`.
    Union,
    /// `trait`.
    Trait,
    /// `impl`.
    Impl,
    /// `fn`.
    Fn,
    /// `mod`.
    Mod,
    /// A macro call (`inventory::submit! { … }`) or a `macro_rules!`.
    Macro,
    /// Anything else (an `extern "C" { … }` block).
    Other,
}

impl ItemKind {
    /// Returns the keyword a report names the item by.
    pub fn word(self) -> &'static str {
        match self {
            ItemKind::Use => "use",
            ItemKind::Const => "const",
            ItemKind::Static => "static",
            ItemKind::TypeAlias => "type",
            ItemKind::Struct => "struct",
            ItemKind::Enum => "enum",
            ItemKind::Union => "union",
            ItemKind::Trait => "trait",
            ItemKind::Impl => "impl",
            ItemKind::Fn => "fn",
            ItemKind::Mod => "mod",
            ItemKind::Macro => "macro",
            ItemKind::Other => "item",
        }
    }
}

/// A top-level item of a Rust file.
#[derive(Debug)]
pub struct TopItem {
    /// 1-based line of the item itself, below its attributes.
    pub line: usize,
    /// What the item is.
    pub kind: ItemKind,
    /// The name the item declares; for an `impl`, the type it is for.
    ///
    /// `impl<T> Trait for Foo<T>` gives `Foo`, `const _` gives `_`; an item
    /// naming nothing (`use`, a macro call) gives an empty name.
    pub name: String,
    /// No visibility at all: not `pub`, nor `pub(crate)`, `pub(super)`, ….
    pub private: bool,
    /// Under `#[cfg(test)]` or `#[cfg(all(test, …))]`.
    pub cfg_test: bool,
    /// A `mod` whose body is written in place (`mod x { … }`), not in a file
    /// of its own (`mod x;`).
    pub inline: bool,
    /// The item's code, its outer attributes included, with comments and the
    /// contents of literals blanked.
    pub code: String,
}

impl TopItem {
    /// Returns `fn name`, `struct Name`, `impl Name`, … as a report shows it.
    pub fn label(&self) -> String {
        if self.name.is_empty() {
            self.kind.word().to_owned()
        } else {
            format!("{} {}", self.kind.word(), self.name)
        }
    }

    /// Tells whether this is a test module: an inline `#[cfg(test)] mod x
    /// { … }`, or `#[cfg(test)] mod tests;` declaring the file of the tests.
    pub fn is_test_module(&self) -> bool {
        self.kind == ItemKind::Mod && self.cfg_test && (self.inline || self.name == "tests")
    }

    /// Tells whether this is an inline `#[cfg(test)] mod x { … }`.
    pub fn is_inline_test_module(&self) -> bool {
        self.kind == ItemKind::Mod && self.cfg_test && self.inline
    }
}

/// Reads the top-level items of a Rust file, in order.
///
/// Comments and the contents of string and character literals are blanked
/// first, so a brace, a `;` or a line at column 0 inside one is not read as
/// code. An item runs from its first token to the `;` that ends it at bracket
/// depth 0 or, for an item with a body (`fn`, `impl`, `struct … { }`,
/// `mod … { }`, a macro call in braces), to the brace that closes that body.
/// A `const`, `static`, `type` or `use` always runs to its `;`, so
/// `const X: Foo = Foo { … };` is one item. Inner attributes (`#![…]`) are
/// skipped; outer attributes belong to the item below them.
pub fn top_level_items(text: &str) -> Vec<TopItem> {
    let code = blank_literals(&strip_rust_comments(text));
    let chars: Vec<char> = code.chars().collect();
    let newlines = |range: &[char]| range.iter().filter(|&&ch| ch == '\n').count();
    let mut items = Vec::new();
    let mut attrs: Vec<String> = Vec::new();
    let mut line = 1usize;
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() || c == ';' {
            if c == '\n' {
                line += 1;
            }
            i += 1;
            continue;
        }
        if c == '#' {
            let inner = chars.get(i + 1) == Some(&'!');
            let open = if inner { i + 2 } else { i + 1 };
            if chars.get(open) == Some(&'[') {
                let end = closing_bracket(&chars, open);
                line += newlines(&chars[i..end]);
                if !inner {
                    attrs.push(chars[i..end].iter().collect());
                }
                i = end;
                continue;
            }
        }

        // An item starts here. Its header runs to the first `{` or `;` outside
        // parentheses and brackets.
        let mut depth = 0usize;
        let mut stop = chars.len();
        for (j, &ch) in chars.iter().enumerate().skip(i) {
            match ch {
                '(' | '[' => depth += 1,
                ')' | ']' => depth = depth.saturating_sub(1),
                '{' | ';' if depth == 0 => {
                    stop = j;
                    break;
                }
                _ => {}
            }
        }
        let header: String = chars[i..stop].iter().collect();
        let (kind, name, private) = classify_item(&header);
        let to_semicolon = matches!(
            kind,
            ItemKind::Const | ItemKind::Static | ItemKind::TypeAlias | ItemKind::Use
        );
        let mut depth = 0usize;
        let mut end = chars.len();
        for (j, &ch) in chars.iter().enumerate().skip(i) {
            match ch {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 && ch == '}' && !to_semicolon {
                        end = j + 1;
                        break;
                    }
                }
                ';' if depth == 0 => {
                    end = j + 1;
                    break;
                }
                _ => {}
            }
        }
        items.push(TopItem {
            line,
            kind,
            name,
            private,
            cfg_test: attrs.iter().any(|attr| is_cfg_test(attr)),
            inline: kind == ItemKind::Mod && chars.get(stop) == Some(&'{'),
            code: attrs.concat() + &chars[i..end].iter().collect::<String>(),
        });
        attrs.clear();
        line += newlines(&chars[i..end]);
        i = end;
    }
    items
}

/// Reads what an item's header (its text up to its body or its `;`)
/// declares: its kind, its name, and whether it has no visibility at all.
pub fn classify_item(header: &str) -> (ItemKind, String, bool) {
    let header = header.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut rest = header.as_str();
    let private = leading_ident(rest) != "pub";
    if !private {
        rest = rest["pub".len()..].trim_start();
        if rest.starts_with('(') {
            rest = rest.find(')').map_or("", |at| &rest[at + 1..]).trim_start();
        }
    }
    loop {
        let word = leading_ident(rest);
        let after = rest[word.len()..].trim_start();
        let next = leading_ident(after);
        let kind = match word {
            "default" | "async" | "unsafe" | "safe" => {
                rest = after;
                continue;
            }
            "const" if matches!(next, "fn" | "unsafe" | "async" | "extern") => {
                rest = after;
                continue;
            }
            "extern" if next == "crate" => return (ItemKind::Use, String::new(), private),
            "extern" => {
                // `extern "C" fn …` is a function; `extern "C" { … }` a block.
                let after_abi = match after.strip_prefix('"') {
                    Some(abi) => abi.find('"').map_or("", |at| &abi[at + 1..]).trim_start(),
                    None => after,
                };
                if leading_ident(after_abi).is_empty() {
                    return (ItemKind::Other, String::new(), private);
                }
                rest = after_abi;
                continue;
            }
            "impl" => return (ItemKind::Impl, impl_self_type(after), private),
            "const" => ItemKind::Const,
            "static" => ItemKind::Static,
            "type" => ItemKind::TypeAlias,
            "struct" => ItemKind::Struct,
            "enum" => ItemKind::Enum,
            "union" => ItemKind::Union,
            "trait" => ItemKind::Trait,
            "fn" => ItemKind::Fn,
            "mod" => ItemKind::Mod,
            "use" => return (ItemKind::Use, String::new(), private),
            "macro_rules" => return (ItemKind::Macro, String::new(), private),
            _ if !word.is_empty() && (after.starts_with('!') || after.starts_with("::")) => {
                return (ItemKind::Macro, String::new(), private);
            }
            _ => return (ItemKind::Other, String::new(), private),
        };
        let name = if kind == ItemKind::Static && next == "mut" {
            leading_ident(after["mut".len()..].trim_start())
        } else {
            next
        };
        return (kind, name.to_owned(), private);
    }
}

/// Returns the name of the type an `impl` is for, from the header after `impl`.
///
/// `<T> Trait for Foo<T>` gives `Foo`, `Foo` gives `Foo`, `<T> Trait for
/// &mut a::Bar` gives `Bar`.
pub fn impl_self_type(after_impl: &str) -> String {
    let mut rest = after_impl.trim_start();
    // Skip the impl's own generics; `->` inside them is no closing angle.
    if rest.starts_with('<') {
        let mut depth = 0i32;
        for (at, ch) in rest.char_indices() {
            match ch {
                '<' => depth += 1,
                '>' if !rest[..at].ends_with('-') => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                rest = &rest[at + 1..];
                break;
            }
        }
    }
    // The type after ` for ` at angle depth 0, if this is a trait impl.
    let mut depth = 0i32;
    for (at, ch) in rest.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' if !rest[..at].ends_with('-') => depth -= 1,
            _ => {}
        }
        if depth == 0 && rest[at..].starts_with(" where ") {
            rest = &rest[..at];
            break;
        }
        if depth == 0 && rest[at..].starts_with(" for ") {
            rest = &rest[at + " for ".len()..];
            break;
        }
    }
    let mut ty = rest.trim_start();
    loop {
        let before = ty;
        ty = ty.trim_start_matches(['&', '!']).trim_start();
        if let Some(lifetime) = ty.strip_prefix('\'') {
            ty = lifetime[leading_ident(lifetime).len()..].trim_start();
        }
        for word in ["mut", "dyn"] {
            if leading_ident(ty) == word {
                ty = ty[word.len()..].trim_start();
            }
        }
        if ty == before {
            break;
        }
    }
    let path: String = ty
        .chars()
        .take_while(|&ch| is_ident_char(ch) || ch == ':')
        .collect();
    path.rsplit("::").next().unwrap_or_default().to_owned()
}

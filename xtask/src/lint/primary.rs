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

//! Joins a file's items into families, to find its primary types.

use std::path::Path;

use super::allowlist::{add, Violations};
use super::code::{closing_bracket, is_ident_char};
use super::items::{ItemKind, TopItem};
use super::order::order_scanned_files;

/// Names a file's primary types when it holds more than one, at the line of
/// the first, each by its representative (see [`type_families`]).
pub fn extra_primary_types(items: &[TopItem]) -> Option<(usize, String)> {
    let families = type_families(items);
    if families.len() < 2 {
        return None;
    }
    let listed: Vec<String> = families
        .iter()
        .map(|ty| format!("`{}` (line {})", ty.label(), ty.line))
        .collect();
    let message = format!("{} primary types: {}", families.len(), listed.join(", "));
    Some((families[0].line, message))
}

/// Collects, per non-test file under `root`, its primary types when it holds
/// more than one ([`extra_primary_types`]), keyed by the file.
///
/// A file's top-level items are joined into families by naming: a type and
/// its impls are one node, and any item (another type, a free `fn`, a
/// `const`, a `static`, an alias, a macro call) that names a type as a whole
/// word joins it — comments and literal contents blanked, `use` and `mod`
/// items and `#[cfg(test)]` ones aside, an enum's variant names not read as
/// types. A family is a group holding a type with a visibility; one is the
/// file's primary type with what serves it.
pub fn primary_type_violations(root: &Path) -> Violations {
    let mut violations = Violations::new();
    for (path, items) in order_scanned_files(root) {
        if let Some((_, message)) = extra_primary_types(&items) {
            add(&mut violations, path, message);
        }
    }
    violations
}

/// Returns the families of a file's top-level `items`, each by its
/// representative, in line order.
///
/// Every item but a `use`, a `mod` and a `#[cfg(test)]` one is a node, a
/// type's impls joined to it; an item naming a type joins it. A family is a
/// connected group holding a non-private type; its representative is its
/// first non-private type that no other type of the group names, else its
/// first non-private type.
pub fn type_families(items: &[TopItem]) -> Vec<&TopItem> {
    let is_type = |item: &TopItem| {
        matches!(
            item.kind,
            ItemKind::Struct | ItemKind::Enum | ItemKind::Union | ItemKind::Trait
        )
    };
    let items: Vec<&TopItem> = items
        .iter()
        .filter(|item| !item.cfg_test && !matches!(item.kind, ItemKind::Use | ItemKind::Mod))
        .collect();
    let enums: Vec<&str> = (items.iter())
        .filter(|item| item.kind == ItemKind::Enum)
        .map(|item| item.name.as_str())
        .collect();
    // Each item's node: a type's own; an impl joins the type it is for, when
    // the file declares it; any other item a node of its own.
    let types: Vec<usize> = (0..items.len()).filter(|&i| is_type(items[i])).collect();
    let node_of = |i: usize| -> usize {
        if items[i].kind == ItemKind::Impl {
            if let Some(&ty) = types.iter().find(|&&ty| items[ty].name == items[i].name) {
                return ty;
            }
        }
        i
    };
    let mut parent: Vec<usize> = (0..items.len()).collect();
    // `named[ty]`: whether another type of the file (its declaration or its
    // impls) names `ty`.
    let mut named = vec![false; items.len()];
    for (i, item) in items.iter().enumerate() {
        let node = node_of(i);
        let code = if item.kind == ItemKind::Enum {
            blank_variant_names(&item.code)
        } else {
            item.code.clone()
        };
        for &ty in &types {
            if ty == node || !names_type(&code, &items[ty].name, &enums) {
                continue;
            }
            let (a, b) = (find_group(&mut parent, node), find_group(&mut parent, ty));
            parent[a] = b;
            if is_type(items[node]) {
                named[ty] = true;
            }
        }
    }
    let mut families: Vec<&TopItem> = Vec::new();
    let mut seen: Vec<usize> = Vec::new();
    for &ty in &types {
        let group = find_group(&mut parent, ty);
        if seen.contains(&group) {
            continue;
        }
        let members: Vec<usize> = (types.iter().copied())
            .filter(|&other| !items[other].private && find_group(&mut parent, other) == group)
            .collect();
        let Some(&first) = members.first() else {
            continue;
        };
        seen.push(group);
        let representative = (members.iter().copied())
            .find(|&other| !named[other])
            .unwrap_or(first);
        families.push(items[representative]);
    }
    families.sort_by_key(|item| item.line);
    families
}

/// Tells whether `code` names the type `word`: as a whole identifier, not as
/// a variant of an enum of `enums` (`Kind::Word`).
pub fn names_type(code: &str, word: &str, enums: &[&str]) -> bool {
    code.match_indices(word).any(|(at, _)| {
        let before = code[..at].chars().next_back();
        let after = code[at + word.len()..].chars().next();
        let whole = !before.is_some_and(is_ident_char) && !after.is_some_and(is_ident_char);
        let variant = (code[..at].strip_suffix("::"))
            .is_some_and(|path| enums.iter().any(|name| names_suffix(path, name)));
        whole && !variant
    })
}

/// Tells whether `path` ends with the whole identifier `name`.
fn names_suffix(path: &str, name: &str) -> bool {
    path.strip_suffix(name)
        .is_some_and(|rest| !rest.chars().next_back().is_some_and(is_ident_char))
}

/// Returns an enum's `code` with each variant's name blanked where it is
/// declared: what a variant is called names no type.
fn blank_variant_names(code: &str) -> String {
    let chars: Vec<char> = code.chars().collect();
    let Some(open) = chars.iter().position(|&ch| ch == '{') else {
        return code.to_owned();
    };
    let mut out = chars.clone();
    let mut depth = 0usize;
    let mut expect_name = true;
    let mut i = open + 1;
    while i < chars.len() {
        let ch = chars[i];
        match ch {
            '#' if depth == 0 && chars.get(i + 1) == Some(&'[') => {
                i = closing_bracket(&chars, i + 1);
                continue;
            }
            '(' | '[' | '{' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            '}' if depth == 0 => break,
            '}' => depth -= 1,
            ',' if depth == 0 => expect_name = true,
            _ if expect_name && depth == 0 && is_ident_char(ch) => {
                let start = i;
                while i < chars.len() && is_ident_char(chars[i]) {
                    i += 1;
                }
                out[start..i].iter_mut().for_each(|blank| *blank = ' ');
                expect_name = false;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out.into_iter().collect()
}

/// Finds the group `at` belongs to, halving the path to it on the way.
fn find_group(parent: &mut [usize], mut at: usize) -> usize {
    while parent[at] != at {
        parent[at] = parent[parent[at]];
        at = parent[at];
    }
    at
}

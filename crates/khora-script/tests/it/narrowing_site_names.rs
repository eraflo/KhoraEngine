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

//! The sites of a `match`, an `if (var …)` and a `while (var …)` follow the
//! site grammar `bytecode/sites.rs` documents — a `match` step, an arm branch
//! `arm.hash8[#n]` — and the block a bound local is declared in names no
//! site: it is a local's scope, not a place a frame can stand.
//!
//! ```text
//! site   := "entry" | path [ ":" point ]
//! path   := step ( "/" branch "/" step )*
//! step   := kind "." hash8 [ "#" n ]
//! branch := "then" | "else" | "body" | "arm." hash8 [ "#" n ]
//! point  := "head" | "await" [ "#" n ] | "call." callee [ "#" n ]
//! ```

use khora_script::{check, compile, lex, parse, Program};

fn build(source: &str) -> Program {
    let parsed = parse(lex(source).tokens);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let checked = check(&parsed.module);
    assert!(
        !checked.diagnostics.iter().any(|d| d.is_error()),
        "{:?}",
        checked.diagnostics
    );
    let compiled = compile(&parsed.module);
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    compiled.program
}

fn ordinal(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_ascii_digit()) && !text.starts_with('0')
}

fn hash8_with_ordinal(text: &str) -> bool {
    let (hash, n) = match text.split_once('#') {
        Some((hash, n)) => (hash, Some(n)),
        None => (text, None),
    };
    hash.len() == 8 && hash.chars().all(|c| c.is_ascii_hexdigit()) && n.is_none_or(ordinal)
}

fn step(text: &str) -> bool {
    text.split_once('.').is_some_and(|(kind, rest)| {
        matches!(
            kind,
            "let"
                | "expr"
                | "if"
                | "while"
                | "for"
                | "foreach"
                | "return"
                | "break"
                | "continue"
                | "become"
                | "match"
                | "block"
        ) && hash8_with_ordinal(rest)
    })
}

fn branch(text: &str) -> bool {
    matches!(text, "then" | "else" | "body")
        || text.strip_prefix("arm.").is_some_and(hash8_with_ordinal)
}

/// Whether `name` is a site name of the grammar.
fn follows_the_grammar(name: &str) -> bool {
    if name == "entry" {
        return true;
    }
    let (path, point) = match name.split_once(':') {
        Some((path, point)) => (path, Some(point)),
        None => (name, None),
    };
    let pieces: Vec<&str> = path.split('/').collect();
    let path_ok = pieces.len() % 2 == 1
        && pieces.iter().enumerate().all(|(index, piece)| {
            if index % 2 == 0 {
                step(piece)
            } else {
                branch(piece)
            }
        });
    let point_ok = match point {
        None | Some("head") => true,
        Some(point) => {
            let (body, n) = match point.split_once('#') {
                Some((body, n)) => (body, Some(n)),
                None => (point, None),
            };
            (body == "await" || body.strip_prefix("call.").is_some_and(|c| !c.is_empty()))
                && n.is_none_or(ordinal)
        }
    };
    path_ok && point_ok
}

/// **Every narrowing form, nested.** Each site name of a body using `match`
/// (nested in an arm of another over the same subject, with a repeated
/// pattern), `if (var …) … else if (var …)` and `while (var …)` is a name of
/// the grammar, unique in its function, and the bound locals' own block
/// (`…/bound`) appears in no site's name.
#[test]
fn the_sites_of_the_narrowing_forms_follow_the_grammar() {
    let program = build(
        "behavior Guard {
             int landed = 0;
             int? best;
             int left = 1;
             int? Next() {
                 if (left == 0) { return null; }
                 left -= 1;
                 return 5;
             }
             async void Attack(int? a) {
                 if (var x = a) {
                     int y = 2;
                     await 1.0s;
                     landed = x + y;
                 } else if (var x = best) {
                     await 1.0s;
                     landed = x;
                 }
                 while (var t = Next()) {
                     await 1.0s;
                     if (t > 3) { break; }
                 }
                 match (a) {
                     int t => {
                         match (a) {
                             int t => { await 1.0s; landed = t; }
                             _ => { await 1.0s; }
                         }
                     }
                     null => { await 1.0s; }
                 }
                 match (best) {
                     _ => { await 1.0s; }
                     _ => { await 1.0s; }
                 }
             }
         }",
    );
    let attack = program.function("Guard.Attack").expect("`Attack` compiled");

    let mut names: Vec<&str> = attack.sites.iter().map(|s| s.name.as_str()).collect();
    for name in &names {
        assert!(follows_the_grammar(name), "`{name}` is not a site name");
        assert!(!name.contains("bound"), "`{name}` names a bound block");
    }
    assert!(
        names.iter().any(|name| name
            .split(['/', ':'])
            .any(|piece| piece.starts_with("arm.") && piece.ends_with("#1"))),
        "the repeated `_` arm is told apart: {names:?}"
    );
    let all = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), all, "each site is named once");
}

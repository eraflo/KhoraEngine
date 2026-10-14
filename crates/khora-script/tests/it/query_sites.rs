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

//! A query is a statement of its own kind, so its sites carry their own name.
//!
//! `foreach (Health h, Regen r) with Player { … }` and `foreach (var x in xs)
//! { … }` share a keyword and nothing else: the query's step is a `query`
//! step, keyed by its header — the bindings, `with` and `without` — and an
//! array `foreach` keeps exactly the names it had before queries existed, so
//! a frame frozen in one by an older build is still found.

use khora_script::vm::SiteKind;
use khora_script::{compile_module, Function, MemoryLoader, Program, ENGINE_COMPONENTS_MODULE};

/// The engine module, as the mirror would serve it.
const ENGINE: &str = "
component Health { int current; int max; }
component Regen { float rate; }
component Player { int team; }
component Dead { float since; }
";

fn build(main: &str) -> Program {
    let loader = MemoryLoader::new()
        .with(ENGINE_COMPONENTS_MODULE, ENGINE)
        .with(
            "main.erg",
            format!("import \"{ENGINE_COMPONENTS_MODULE}\";\n{main}"),
        );
    let result = compile_module(&loader, "main.erg");
    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "should compile:\n{main}\n{errors:?}");
    result.program.expect("a program with no errors")
}

fn function<'a>(program: &'a Program, name: &str) -> &'a Function {
    program
        .function(name)
        .unwrap_or_else(|| panic!("`{name}` is a function of the program"))
}

fn names(function: &Function) -> Vec<String> {
    function
        .sites
        .iter()
        .map(|site| site.name.clone())
        .collect()
}

fn loop_heads(function: &Function) -> Vec<String> {
    function
        .sites
        .iter()
        .filter(|site| site.kind == SiteKind::LoopHead)
        .map(|site| site.name.clone())
        .collect()
}

/// `Census.Tally` with `body` as its statements.
fn census(body: &str) -> Program {
    build(&format!(
        "behavior Census {{
             int total = 0;
             void Tally() {{
                 {body}
             }}
         }}"
    ))
}

/// **An array `foreach` keeps its names.** The site names of a body iterating
/// an array are pinned as they were before the query statement existed: the
/// step kind, the hash of its header and the branch into its body.
#[test]
fn an_array_foreach_keeps_its_site_names() {
    let program = census(
        "int[] xs = [1, 2, 3];
         foreach (int x in xs) {
             total += x;
         }",
    );

    assert_eq!(
        names(function(&program, "Census.Tally")),
        [
            "entry",
            "let.55e281b2",
            "foreach.ea09abd2",
            "foreach.ea09abd2:copy",
            "foreach.ea09abd2:head",
            "foreach.ea09abd2/body/expr.5c583926",
        ],
        "the array foreach's site names, as pinned"
    );
}

/// **A query is a `query` step.** The loop head of a query, and every site of
/// its body, is named under a `query.` step — never a `foreach.` one.
#[test]
fn a_query_names_its_sites_under_a_query_step() {
    let program = census(
        "foreach (Health h, Regen r) with Player without Dead {
             total += h.current;
         }",
    );
    let tally = function(&program, "Census.Tally");

    let heads = loop_heads(tally);
    assert_eq!(heads.len(), 1, "one loop head, the query's: {heads:?}");
    assert!(
        heads[0].starts_with("query."),
        "the query's loop head is a `query` step: {heads:?}"
    );
    for name in names(tally) {
        assert!(
            !name.contains("foreach."),
            "no site of a query is named as an array foreach: `{name}`"
        );
    }
}

/// **A query is keyed by its header.** Editing its body leaves its loop
/// head's name alone; changing a binding, a `with` or a `without` renames it.
#[test]
fn a_query_site_follows_its_header_not_its_body() {
    let head = |query: &str| {
        let program = census(query);
        let heads = loop_heads(function(&program, "Census.Tally"));
        assert_eq!(heads.len(), 1, "one loop head: {heads:?}");
        heads[0].clone()
    };

    let base = head("foreach (Health h, Regen r) with Player without Dead { total += h.current; }");

    assert_eq!(
        base,
        head(
            "foreach (Health h, Regen r) with Player without Dead { total += h.max; total += 1; }"
        ),
        "an edited body keeps the query's name"
    );
    for other in [
        "foreach (Health h) with Player without Dead { total += h.current; }",
        "foreach (Health k, Regen r) with Player without Dead { total += k.current; }",
        "foreach (Health h, Regen r) without Dead { total += h.current; }",
        "foreach (Health h, Regen r) with Player { total += h.current; }",
        "foreach (Health h, Regen? r) with Player without Dead { total += h.current; }",
    ] {
        assert_ne!(
            base,
            head(other),
            "another header renames the query:\n{other}"
        );
    }
}

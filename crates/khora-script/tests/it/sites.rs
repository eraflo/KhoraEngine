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

//! Named sites: where a frame can stand, named by what the source says.
//!
//! A statement is keyed by its own tokens — a compound one by its header only —
//! so inserting, deleting or editing an unrelated statement leaves a site's
//! name alone, and a frame frozen there can be found again in edited code.

use std::collections::BTreeSet;

use khora_core::script::{FrozenValue, PendingBody};
use khora_script::arena::PersistentStore;
use khora_script::dispatch::handler_name;
use khora_script::vm::{Site, SiteKind, Suspension};
use khora_script::{
    check, compile, lex, parse, Function, Host, Instruction, Machine, Program, Run, Value,
};

fn build(source: &str) -> Program {
    let lexed = lex(source);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    let parsed = parse(lexed.tokens);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let checked = check(&parsed.module);
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
    let compiled = compile(&parsed.module);
    assert!(
        compiled.diagnostics.is_empty(),
        "{:?}",
        compiled.diagnostics
    );
    compiled.program
}

/// A behavior `Guard` with one `int fired` and `member`.
fn guard(member: &str) -> Program {
    build(&format!(
        "behavior Guard {{
             int fired = 0;
             {member}
         }}"
    ))
}

fn function<'a>(program: &'a Program, name: &str) -> &'a Function {
    program
        .function(name)
        .unwrap_or_else(|| panic!("`{name}` is a function of the program"))
}

/// The sites of `kind`'s variant, in program-counter order.
fn sites_of(function: &Function, kind: fn(&SiteKind) -> bool) -> Vec<&Site> {
    function
        .sites
        .iter()
        .filter(|site| kind(&site.kind))
        .collect()
}

fn is_await(kind: &SiteKind) -> bool {
    *kind == SiteKind::Await
}

fn is_loop_head(kind: &SiteKind) -> bool {
    *kind == SiteKind::LoopHead
}

fn names(sites: &[&Site]) -> Vec<String> {
    sites.iter().map(|site| site.name.clone()).collect()
}

fn all_names(function: &Function) -> BTreeSet<String> {
    function
        .sites
        .iter()
        .map(|site| site.name.clone())
        .collect()
}

/// The `(name, type)` of every local a site lists.
fn locals(site: &Site) -> BTreeSet<(String, String)> {
    site.locals
        .iter()
        .map(|local| (local.name.clone(), local.ty.clone()))
        .collect()
}

fn pairs(list: &[(&str, &str)]) -> BTreeSet<(String, String)> {
    list.iter()
        .map(|(name, ty)| ((*name).to_owned(), (*ty).to_owned()))
        .collect()
}

/// Whether `name` is a site name of the grammar:
///
/// ```text
/// site   := "entry" | path [ ":" point ]
/// path   := step ( "/" branch "/" step )*
/// step   := kind "." hash8 [ "#" n ]
/// branch := "then" | "else" | "body"
/// point  := "head" | "await" [ "#" n ] | "call." callee [ "#" n ]
/// ```
fn follows_the_grammar(name: &str) -> bool {
    if name == "entry" {
        return true;
    }
    let (path, point) = match name.split_once(':') {
        Some((path, point)) => (path, Some(point)),
        None => (name, None),
    };
    let ordinal = |text: &str| {
        !text.is_empty() && text.chars().all(|c| c.is_ascii_digit()) && !text.starts_with('0')
    };
    let step = |text: &str| {
        let (body, n) = match text.split_once('#') {
            Some((body, n)) => (body, Some(n)),
            None => (text, None),
        };
        let Some((kind, hash)) = body.split_once('.') else {
            return false;
        };
        matches!(
            kind,
            "let" | "expr" | "if" | "while" | "for" | "return" | "become" | "block"
        ) && hash.len() == 8
            && hash.chars().all(|c| c.is_ascii_hexdigit())
            && n.is_none_or(ordinal)
    };
    let pieces: Vec<&str> = path.split('/').collect();
    let path_ok = pieces.len() % 2 == 1
        && pieces.iter().enumerate().all(|(index, piece)| {
            if index % 2 == 0 {
                step(piece)
            } else {
                matches!(*piece, "then" | "else" | "body")
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

const SPREAD: &str = "async void Attack(int times) {
                          await 0.5s;
                          if (times > 0) {
                              await 0.25s;
                              fired += 1;
                          } else {
                              fired -= 1;
                          }
                          int i = 0;
                          while (i < times) {
                              await 0.1s;
                              i = i + 1;
                          }
                          for (int j = 0; j < 2; j = j + 1) {
                              await 0.1s;
                          }
                      }";

// ─── Every place a frame can stand is named ─────────────────────────────────

/// **An `await` is a place a frame stands.** Every `await` instruction has a
/// site of kind `Await` just after it — where the frame resumes — whether it
/// sits at the top of the body, in a branch, or in a loop.
#[test]
fn every_await_has_a_named_site() {
    let program = guard(SPREAD);
    let attack = function(&program, "Guard.Attack");

    let awaits: Vec<usize> = attack
        .code
        .iter()
        .enumerate()
        .filter(|(_, instruction)| matches!(instruction, Instruction::Await { .. }))
        .map(|(pc, _)| pc + 1)
        .collect();
    assert_eq!(awaits.len(), 4, "the premise: four awaits");

    let sites: Vec<u32> = sites_of(attack, is_await)
        .iter()
        .map(|site| site.pc)
        .collect();
    let expected: Vec<u32> = awaits.iter().map(|pc| *pc as u32).collect();
    assert_eq!(sites, expected, "one `Await` site just after each await");
    for site in sites_of(attack, is_await) {
        assert!(
            site.name.contains(":await"),
            "an await's site is named as one: {}",
            site.name
        );
    }
}

/// Every site name follows the grammar, is unique within its function, and
/// the sites are sorted by program counter; every function has one `entry`.
#[test]
fn every_site_is_named_by_the_grammar_once_per_function() {
    let program = guard(SPREAD);

    for function in &program.functions {
        let entries: Vec<&Site> = sites_of(function, |kind| *kind == SiteKind::Entry);
        assert_eq!(
            entries
                .iter()
                .map(|site| (site.name.as_str(), site.pc))
                .collect::<Vec<_>>(),
            [("entry", 0)],
            "`{}` has one entry, at the top",
            function.name
        );
        assert_eq!(
            all_names(function).len(),
            function.sites.len(),
            "`{}` names each site once: {:?}",
            function.name,
            function.sites
        );
        assert!(
            function
                .sites
                .windows(2)
                .all(|pair| pair[0].pc <= pair[1].pc),
            "`{}`'s sites are sorted by pc",
            function.name
        );
        for site in &function.sites {
            assert!(
                follows_the_grammar(&site.name),
                "`{}`: `{}` is not a site name",
                function.name,
                site.name
            );
        }
    }
    assert!(
        function(&program, "Guard.Attack").sites.len() > 4,
        "the member has sites beyond its entry and awaits"
    );
}

// ─── Names come from what the source says, not where it sits ────────────────

/// **The common edit.** A line added earlier in a sequence renames nothing
/// that was already there.
#[test]
fn a_site_keeps_its_name_when_a_statement_is_inserted_before_it() {
    let before = guard(
        "async void Attack() {
             fired += 1;
             await 1.0s;
             fired += 2;
         }",
    );
    let after = guard(
        "async void Attack() {
             fired += 5;
             fired += 1;
             await 1.0s;
             fired += 2;
         }",
    );
    let before = function(&before, "Guard.Attack");
    let after = function(&after, "Guard.Attack");

    assert_eq!(
        sites_of(before, is_await).len(),
        1,
        "the premise: one await"
    );
    assert_eq!(
        names(&sites_of(before, is_await)),
        names(&sites_of(after, is_await)),
        "the await keeps its name"
    );
    let lost: Vec<String> = all_names(before)
        .difference(&all_names(after))
        .cloned()
        .collect();
    assert!(lost.is_empty(), "renamed by the insertion: {lost:?}");
}

/// **A compound statement is keyed by its header.** An edit inside an `if`'s
/// branch or a loop's body, away from the site, leaves the `if`, the loop, the
/// head, and the awaits inside them with the names they had.
#[test]
fn a_site_keeps_its_name_when_its_enclosing_body_is_edited_elsewhere() {
    let before = guard(
        "async void Patrol(int laps) {
             if (laps > 0) {
                 fired += 1;
                 await 1.0s;
                 fired += 2;
             }
             while (fired < 10) {
                 await 0.5s;
                 fired += 1;
             }
         }",
    );
    let after = guard(
        r#"async void Patrol(int laps) {
               if (laps > 0) {
                   fired += 1;
                   await 1.0s;
                   fired += 7;
                   Log("lap");
               }
               while (fired < 10) {
                   await 0.5s;
                   fired += 3;
               }
           }"#,
    );
    let before = function(&before, "Guard.Patrol");
    let after = function(&after, "Guard.Patrol");

    assert_eq!(
        sites_of(before, is_await).len(),
        2,
        "the premise: two awaits"
    );
    assert_eq!(
        names(&sites_of(before, is_await)),
        names(&sites_of(after, is_await)),
        "the awaits keep their names"
    );
    assert_eq!(
        names(&sites_of(before, is_loop_head)),
        names(&sites_of(after, is_loop_head)),
        "the loop's head keeps its name"
    );
    let compound = |function: &Function| -> BTreeSet<String> {
        function
            .sites
            .iter()
            .filter(|site| site.name.starts_with("if.") && !site.name.contains('/'))
            .map(|site| site.name.clone())
            .collect()
    };
    assert!(
        !compound(before).is_empty(),
        "the `if` has a site of its own"
    );
    assert_eq!(compound(before), compound(after), "the `if` keeps its name");
}

/// A site is renamed when what it stands on changes: the statement's own text,
/// or the header of a compound statement around it.
#[test]
fn a_site_changes_name_when_its_statement_changes() {
    let before = guard(
        "async void Patrol(int laps) {
             await 0.5s;
             if (laps > 0) {
                 await 1.0s;
             }
         }",
    );
    let after = guard(
        "async void Patrol(int laps) {
             await 0.75s;
             if (laps > 1) {
                 await 1.0s;
             }
         }",
    );
    let before = names(&sites_of(function(&before, "Guard.Patrol"), is_await));
    let after = names(&sites_of(function(&after, "Guard.Patrol"), is_await));

    assert_eq!(before.len(), 2, "the premise: two awaits");
    assert_eq!(after.len(), 2);
    assert_ne!(before[0], after[0], "the await's own text changed");
    assert_ne!(
        before[1], after[1],
        "the header of the `if` around the await changed"
    );
}

/// **Two identical statements are two sites.** The second is told apart by
/// its ordinal, `#1`, among siblings saying the same thing — and an unrelated
/// statement inserted around them shifts neither.
#[test]
fn two_identical_statements_have_distinct_sites() {
    let twice = guard(
        "async void Attack() {
             await 1.0s;
             await 1.0s;
         }",
    );
    let spaced = guard(
        "async void Attack() {
             fired += 1;
             await 1.0s;
             fired += 2;
             await 1.0s;
         }",
    );
    let twice = names(&sites_of(function(&twice, "Guard.Attack"), is_await));
    let spaced = names(&sites_of(function(&spaced, "Guard.Attack"), is_await));

    assert_eq!(twice.len(), 2, "the premise: two awaits");
    assert_ne!(twice[0], twice[1], "two sites, two names");
    assert!(
        !twice[0].contains('#') && twice[1].contains("#1"),
        "the second is the first's ordinal 1: {twice:?}"
    );
    assert_eq!(twice, spaced, "an unrelated insertion shifts neither");
}

// ─── What a site holds ──────────────────────────────────────────────────────

/// **The locals in scope, by name and type.** Parameters included, in the
/// registers the calling convention gives them; a local declared in a block
/// that has closed is not in scope after it.
#[test]
fn a_site_lists_its_locals_by_name_and_type() {
    let program = guard(
        r#"async void Aim(int target, float speed) {
               int a = target;
               if (a > 0) {
                   float inner = speed;
                   await 1.0s;
                   fired += 1;
               }
               string label = "x";
               await 2.0s;
           }"#,
    );
    let aim = function(&program, "Guard.Aim");

    let entry = sites_of(aim, |kind| *kind == SiteKind::Entry);
    let entry = entry.first().expect("an entry site");
    assert_eq!(
        locals(entry),
        pairs(&[("target", "int"), ("speed", "float")]),
        "the entry holds the parameters"
    );
    let registers: Vec<(String, u8)> = entry
        .locals
        .iter()
        .map(|local| (local.name.clone(), local.register))
        .collect();
    assert!(
        registers.contains(&("target".to_owned(), 0))
            && registers.contains(&("speed".to_owned(), 1)),
        "parameters sit where the calling convention leaves them: {registers:?}"
    );

    let awaits = sites_of(aim, is_await);
    assert_eq!(awaits.len(), 2, "the premise: two awaits");
    assert_eq!(
        locals(awaits[0]),
        pairs(&[
            ("target", "int"),
            ("speed", "float"),
            ("a", "int"),
            ("inner", "float"),
        ]),
        "inside the branch"
    );
    assert_eq!(
        locals(awaits[1]),
        pairs(&[
            ("target", "int"),
            ("speed", "float"),
            ("a", "int"),
            ("label", "string"),
        ]),
        "after the branch closed: `inner` is out of scope"
    );
}

// ─── Timer names ────────────────────────────────────────────────────────────

/// **A timer is named by what it is.** `"{owner}.__every({secs})"` or
/// `"{owner}.__after({secs})"`, the owner the behavior or one of its states,
/// and `#n` only among the owner's timers of the same kind and interval. So a
/// field added before a timer, or a timer of another kind or interval inserted
/// before it, renames nothing; a second identical timer is told apart by `#1`.
///
/// Every interval below is exact as an `f32`, so its `f64` `Display` is the
/// interval as written.
#[test]
fn timer_names_follow_their_kind_and_interval() {
    let before = build(
        "behavior Ticker {
             int a = 0;
             every 1s { a += 1; }
             state Patrol {
                 int laps = 0;
                 every 0.5s { laps += 1; }
             }
         }",
    );
    let after = build(
        "behavior Ticker {
             int b = 0;
             int a = 0;
             every 2s { a += 5; }
             every 1s { a += 1; }
             int c = 0;
             after 1s { a += 2; }
             every 1s { a += 3; }
             state Patrol {
                 int more = 0;
                 int laps = 0;
                 every 0.25s { laps += 2; }
                 every 0.5s { laps += 1; }
             }
         }",
    );
    let members = |program: &Program| -> Vec<String> {
        program
            .layout("Ticker")
            .expect("the behavior has a layout")
            .timers
            .iter()
            .map(|timer| timer.member.clone())
            .collect()
    };

    assert_eq!(
        members(&before),
        ["Ticker.__every(1)", "Ticker.Patrol.__every(0.5)"]
    );
    assert_eq!(
        members(&after),
        [
            "Ticker.__every(2)",
            "Ticker.__every(1)",
            "Ticker.__after(1)",
            "Ticker.__every(1)#1",
            "Ticker.Patrol.__every(0.25)",
            "Ticker.Patrol.__every(0.5)",
        ],
        "fields and timers of another kind or interval rename nothing; the second \
         identical `every 1s` is `#1`"
    );
    for program in [&before, &after] {
        for member in members(program) {
            assert!(
                program.function(&member).is_some(),
                "`{member}` is the function the timer's body compiled to"
            );
        }
    }
}

// ─── A frozen machine names where it stands ─────────────────────────────────

const AIMER: &str = "behavior Guard {
                         int fired = 0;
                         async void Attack() {
                             await 1.0s;
                             fired += 1;
                         }
                         async void Aim(int by) {
                             by = by + 1;
                             Attack();
                         }
                     }";

/// `Guard.Aim(7)` stopped at the `await` inside `Attack`, frozen.
fn aimer_stopped() -> (Program, Machine, khora_core::script::FrozenMachine) {
    let program = build(AIMER);
    let layout = program.layout("Guard").expect("the behavior has a layout");
    let mut host = Host::new().with_fields(PersistentStore::with_slots(layout.slot_count()));
    let aim = khora_script::dispatch::resolve_member(&program, "Guard", "Aim", &host)
        .expect("the member exists");
    let mut machine = Machine::new(&program, &aim, &[Value::Int(7)]).expect("entry exists");
    assert_eq!(
        machine.run(&program, &mut host, u64::MAX),
        Run::Suspended(Suspension::Awaiting)
    );
    let frozen = machine
        .freeze(&program, PendingBody::Sequence)
        .expect("a machine of this program freezes");
    (program, machine, frozen)
}

/// **A frame names where it stands.** The innermost frame names the `await`
/// site it resumes at; its caller names the site its call returns to; and
/// each frame carries its function's fingerprint.
#[test]
fn a_frozen_frame_names_its_site_and_its_functions_fingerprint() {
    let (program, machine, frozen) = aimer_stopped();
    assert_eq!(frozen.frames.len(), 2, "stopped inside the call");

    let caller = &frozen.frames[0];
    let callee = &frozen.frames[1];
    let aim = function(&program, &caller.function);
    let attack = function(&program, &callee.function);

    let awaiting = attack
        .sites
        .iter()
        .find(|site| site.pc as usize == machine.program_counter());
    assert!(
        awaiting.is_some_and(|site| site.kind == SiteKind::Await && site.name == callee.site),
        "the innermost frame names the await it resumes at: `{}`, sites {:?}",
        callee.site,
        attack.sites
    );
    let returning = aim
        .sites
        .iter()
        .find(|site| u64::from(site.pc) == callee.return_pc);
    assert!(
        returning.is_some_and(
            |site| matches!(site.kind, SiteKind::Return { .. }) && site.name == caller.site
        ),
        "the caller names the site its call returns to: `{}`, sites {:?}",
        caller.site,
        aim.sites
    );

    assert_ne!(
        aim.fingerprint, attack.fingerprint,
        "two different functions"
    );
    assert_eq!(
        (caller.fingerprint, callee.fingerprint),
        (aim.fingerprint, attack.fingerprint),
        "each frame carries its function's fingerprint"
    );
}

/// **What a body was started with is kept.** A parameter is a local the body
/// may reassign, so the frame cannot say what it was called with; the frozen
/// machine records the arguments as they were.
#[test]
fn a_frozen_machine_keeps_the_arguments_its_body_started_with() {
    let (program, _, frozen) = aimer_stopped();
    assert_eq!(
        frozen.frames.first().map(|frame| frame.function.as_str()),
        program
            .function(&handler_name("Guard", "Aim"))
            .map(|function| function.name.as_str()),
        "the premise: the body is `Aim`"
    );

    assert_eq!(
        frozen.arguments,
        [FrozenValue::Int(7)],
        "`by` was 7 when the body started, whatever it holds now"
    );
}

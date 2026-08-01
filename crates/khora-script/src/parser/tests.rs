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

//! Parser tests.

use super::parse;
use crate::ast::*;
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

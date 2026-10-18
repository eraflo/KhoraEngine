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

//! Where an asset type is written: a behavior's field, a component's, a
//! struct's, a local, a parameter, a return — and the two refusals, a bare
//! asset field and a struct named like a kind.

use khora_core::asset::AssetKind;
use khora_core::script::{ErgonType, ScriptValue};
use khora_script::arena::Persisted;
use khora_script::vm::{DeclaredComponent, Program, Value};

use super::{build, refused_at, run_fn, KINDS};
use crate::support::fresh_store;

fn kind_of(spelling: &str) -> AssetKind {
    match spelling {
        "Prefab" => AssetKind::Prefab,
        "Sound" => AssetKind::Sound,
        "Material" => AssetKind::Material,
        "Mesh" => AssetKind::Mesh,
        "Texture" => AssetKind::Texture,
        "Scene" => AssetKind::Scene,
        other => panic!("`{other}` is no asset kind"),
    }
}

fn optional_asset(spelling: &str) -> ErgonType {
    ErgonType::Optional(Box::new(ErgonType::Asset(kind_of(spelling))))
}

fn declared<'p>(program: &'p Program, name: &str) -> &'p DeclaredComponent {
    program
        .declared
        .iter()
        .find(|declared| declared.name == name)
        .unwrap_or_else(|| panic!("`{name}` is declared"))
}

/// The declared type and registered default of `component`'s field `field`.
fn field_of(component: &DeclaredComponent, field: &str) -> (ErgonType, ScriptValue) {
    component
        .fields
        .iter()
        .find(|known| known.name == field)
        .map(|known| (known.ty.clone(), known.default.clone()))
        .unwrap_or_else(|| panic!("`{}` declares `{field}`", component.name))
}

// ─── Fields ─────────────────────────────────────────────────────────────────

/// **A behavior's optional asset field of each kind is accepted**, declared
/// as that kind, registered at `null` — and a fresh instance holds `null`.
#[test]
fn a_behavior_field_of_each_asset_kind_is_declared_and_starts_null() {
    for kind in KINDS {
        let program = build(&format!("behavior Gun {{ {kind}? chosen; }}"));
        assert_eq!(
            field_of(declared(&program, "Gun"), "chosen"),
            (optional_asset(kind), ScriptValue::Null),
            "`{kind}? chosen;`"
        );
        let layout = program.layout("Gun").expect("a layout");
        let slot = layout.slot_of("chosen").expect("a declared field");
        assert_eq!(
            fresh_store(&program, "Gun").get(slot),
            Some(&Persisted::Scalar(Value::Null)),
            "`{kind}? chosen;` starts at null"
        );
    }
}

/// **A component's optional asset field of each kind is accepted** and
/// registered at `null`, as a behavior's is.
#[test]
fn a_component_field_of_each_asset_kind_is_declared_and_starts_null() {
    for kind in KINDS {
        let program = build(&format!(
            "component Loadout {{ {kind}? chosen; int rounds; }}"
        ));
        let loadout = declared(&program, "Loadout");
        assert_eq!(
            field_of(loadout, "chosen"),
            (optional_asset(kind), ScriptValue::Null),
            "`{kind}? chosen;`"
        );
        assert_eq!(
            field_of(loadout, "rounds"),
            (ErgonType::Int, ScriptValue::Int(0)),
            "the other field is untouched"
        );
    }
}

/// **A field may say `= null`**, the one default an asset field can be
/// written with.
#[test]
fn an_asset_field_may_default_to_null() {
    let program = build("behavior Gun { Prefab? bullet = null; }");
    assert_eq!(
        field_of(declared(&program, "Gun"), "bullet"),
        (optional_asset("Prefab"), ScriptValue::Null)
    );
}

/// **A bare asset field is refused** — there is no asset literal, so no
/// default could be written — underlining the field, and the note says to
/// make it optional and choose it in the inspector.
#[test]
fn a_bare_asset_field_is_refused_saying_to_make_it_optional() {
    for kind in KINDS {
        let behavior = format!("behavior Gun {{ {kind} bullet; }}");
        let refusal = refused_at(&behavior, &format!("{kind} bullet;"), &["`bullet`"]);
        let note = refusal.note.clone().unwrap_or_default();
        assert!(
            note.contains(&format!("{kind}?")) && note.contains("inspector"),
            "`{kind} bullet;`: the note says to write `{kind}?` and choose it in the inspector: {note:?}"
        );

        let component = format!("component Loadout {{ {kind} bullet; }}");
        let refusal = refused_at(&component, &format!("{kind} bullet;"), &["`bullet`"]);
        assert!(
            refusal
                .note
                .as_deref()
                .is_some_and(|note| note.contains(&format!("{kind}?"))),
            "a component's bare `{kind}` field: {:?}",
            refusal.note
        );
    }
}

/// **A struct's optional asset field is accepted**, and so is an array of
/// optional assets as a behavior's field: the spellings are types wherever
/// a type is written.
#[test]
fn a_struct_field_and_an_array_field_of_assets_are_accepted() {
    let program = build(
        "struct Look { Mesh? model; Texture? decal; }
         behavior Prop {
             Look look;
             Material?[] skins;
             Sound? hum;
         }",
    );
    let (ty, default) = field_of(declared(&program, "Prop"), "skins");
    assert_eq!(
        ty,
        ErgonType::Array(Box::new(optional_asset("Material"))),
        "`Material?[] skins;`"
    );
    assert_eq!(default, ScriptValue::Array(Vec::new()));
}

// ─── Locals, parameters, returns ────────────────────────────────────────────

/// **An optional asset local starts at `null`**, as any optional does;
/// a bare one with no value is refused — its type has no default.
#[test]
fn an_asset_local_starts_null_and_a_bare_one_needs_a_value() {
    for kind in KINDS {
        let program = build(&format!("fn bool F() {{ {kind}? a; return a == null; }}"));
        assert_eq!(
            run_fn(&program, "F", &[]),
            Value::Bool(true),
            "`{kind}? a;`"
        );

        refused_at(
            &format!("fn void F() {{ {kind} a; }}"),
            &format!("{kind} a;"),
            &["`a` needs a value", &format!("`{kind}`")],
        );
    }
}

/// **Every kind is accepted as a parameter, bare or optional, as a local and
/// as a return type**, and an array of assets as a local.
#[test]
fn every_kind_is_a_type_of_parameters_locals_and_returns() {
    for kind in KINDS {
        build(&format!(
            "fn {kind}? Pick({kind} a, {kind}? b, bool first) {{
                 {kind}? chosen = b;
                 if (first) {{ chosen = a; }}
                 return chosen;
             }}
             fn int Count() {{ {kind}[] none = []; return none.Length; }}"
        ));
    }
}

// ─── Names that belong to the language ──────────────────────────────────────

/// **A struct named like an asset kind is refused** — the name belongs to
/// the language — naming it.
#[test]
fn a_struct_named_like_an_asset_kind_is_refused() {
    for kind in KINDS {
        let source = format!("struct {kind} {{ int n; }}");
        refused_at(&source, &source, &[&format!("`{kind}`")]);
    }
}

/// **A component or a behavior named like an asset kind is refused** â€” the
/// name belongs to the language, as a struct's does. Accepted, it would take
/// the asset type from every field written with it: `Sound? shot;` beside a
/// `component Sound` is refused as "`Sound` is a component".
#[test]
fn a_component_or_a_behavior_named_like_an_asset_kind_is_refused() {
    for kind in KINDS {
        for source in [
            format!("component {kind} {{ int n = 0; }}"),
            format!("behavior {kind} {{ int n; }}"),
        ] {
            refused_at(&source, &source, &[&format!("`{kind}`")]);
        }
    }
}

/// **A component or behavior refused for its asset name takes nothing from
/// the asset type**: the refusal is the one error, and a field, parameter
/// or return written `Sound?` beside it — here or in a module importing
/// it — is still an asset, not "`Sound` is a component".
#[test]
fn a_component_refused_for_an_asset_name_leaves_asset_types_alone() {
    for (declaration, user) in [
        (
            "component Sound { int n = 0; }",
            "behavior Gun { Sound? shot; }",
        ),
        (
            "behavior Sound { int n; }",
            "behavior Gun { Sound[] shots; }",
        ),
        (
            "behavior Sound { int n; }",
            "fn Sound? Pick(Sound? s) { return s; }",
        ),
        ("behavior Sound { int n; }", "struct Loot { Sound? s; }"),
    ] {
        let source = format!("{declaration}\n{user}");
        let result = super::compile(&source);
        let found = super::listed(&source, &result);
        assert_eq!(
            found.len(),
            1,
            "one error, the refusal:\n{source}\n{found:?}"
        );

        let imported = khora_script::compile_module(
            &khora_script::MemoryLoader::new()
                .with("lib.erg", declaration)
                .with("main.erg", format!("import \"lib.erg\";\n{user}")),
            "main.erg",
        );
        let errors: Vec<&str> = imported
            .diagnostics
            .iter()
            .filter(|d| d.is_error())
            .map(|d| d.message.as_str())
            .collect();
        assert_eq!(
            errors.len(),
            1,
            "one error, the refusal in `lib.erg`:\n{user}\n{errors:?}"
        );
    }
}

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

//! The engine's components as Ergon declarations, built from each
//! [`ComponentShape`], so a script's misspelt field is refused at compile time.
//!
//! Served from memory, never a file, so they always describe the running
//! engine. A field Ergon cannot name is kept as a comment naming its Rust type
//! ([`FieldSchema::ty`]), never dropped with its component.
//! See the book: reference/ergon.md, "Writing components".
//!
//! [`ComponentShape`]: khora_data::scene::ComponentShape
//! [`FieldSchema::ty`]: khora_data::scene::FieldSchema::ty

use std::sync::OnceLock;

use khora_data::ecs::ComponentProvenance;
use khora_data::scene::{ComponentRegistration, ComponentShape};
use khora_script::modules::SourceLoader;
use khora_script::TokenKind;

/// Where a script imports the mirrors from: `import "engine/components.erg";`.
///
/// A project's own `component` may not take an engine component's name,
/// imported or not: a name is unique among every component the engine stores.
pub const MIRROR_MODULE: &str = khora_script::ENGINE_COMPONENTS_MODULE;

/// Returns the Ergon source mirroring every registered component.
///
/// Built once: the registry is filled at link time, so it cannot change while
/// the process runs.
pub fn mirror_source() -> &'static str {
    static SOURCE: OnceLock<String> = OnceLock::new();
    SOURCE.get_or_init(generate)
}

/// A loader that answers [`MIRROR_MODULE`] itself and delegates everything else.
///
/// Stacked over whatever serves a project's own scripts:
///
/// ```no_run
/// use khora_io::script::compile::DiskLoader;
/// use khora_script::compile_module;
/// use khora_io::script::mirror::PreludeLoader;
///
/// let loader = PreludeLoader::new(DiskLoader::new("assets/scripts"));
/// compile_module(&loader, "hover.erg");
/// ```
#[derive(Debug, Clone)]
pub struct PreludeLoader<L> {
    inner: L,
}

impl<L> PreludeLoader<L> {
    /// Wraps `inner`, which serves every module but the mirrors.
    pub fn new(inner: L) -> Self {
        Self { inner }
    }

    /// Returns the wrapped loader.
    pub fn inner(&self) -> &L {
        &self.inner
    }
}

impl<L: SourceLoader> SourceLoader for PreludeLoader<L> {
    fn load(&self, path: &str) -> Option<String> {
        if path == MIRROR_MODULE {
            // Ahead of the inner loader: a file left at this path is a copy of
            // an older engine, and must not shadow the running one.
            return Some(mirror_source().to_owned());
        }
        self.inner.load(path)
    }

    fn engine_components(&self) -> &[String] {
        engine_component_names()
    }
}

/// Returns every engine component's name, former name and retired name, sorted.
///
/// The complete list, which the mirror is not. A retired name stays the
/// engine's: a script declaring it would read an old file's cells of the
/// removed type.
pub fn engine_component_names() -> &'static [String] {
    static NAMES: OnceLock<Vec<String>> = OnceLock::new();
    NAMES.get_or_init(|| {
        let mut names: Vec<String> = inventory::iter::<ComponentRegistration>
            .into_iter()
            .flat_map(|registration| {
                std::iter::once(registration.type_name).chain(registration.formerly.iter().copied())
            })
            .chain(
                inventory::iter::<khora_data::scene::RetiredComponent>
                    .into_iter()
                    .map(|retired| retired.name),
            )
            .map(str::to_owned)
            .collect();
        names.sort();
        names.dedup();
        names
    })
}

/// Builds the module.
fn generate() -> String {
    let mut registrations: Vec<&ComponentRegistration> = inventory::iter::<ComponentRegistration>
        .into_iter()
        .collect();
    // Sorted rather than left in link order: the readable copy an IDE keeps
    // would otherwise reshuffle between builds, turning every diff into noise.
    registrations.sort_by_key(|registration| registration.type_name);

    let mut source = String::from(
        "// The engine's components, as Ergon sees them.\n\
         //\n\
         // Generated from the component registry of the engine you are running,\n\
         // and served from memory — there is no file behind this module, so it\n\
         // cannot fall out of step with the Rust it mirrors.\n\
         //\n\
         // A field Ergon cannot yet name is kept below as a comment rather than\n\
         // dropped, so what is missing is visible instead of merely absent.\n",
    );

    for registration in registrations {
        source.push('\n');
        source.push_str(&declaration(registration));
    }
    source
}

/// One component, as a declaration or as the reason there is none.
fn declaration(registration: &ComponentRegistration) -> String {
    let name = registration.type_name;

    let fields = match registration.shape {
        // An enum is a one-of; a mirror with no fields would claim it is a
        // marker, which is a different and false statement.
        ComponentShape::Opaque => {
            return not_mirrored(name, "a one-of rather than a set of fields")
        }
        ComponentShape::Fields(fields) => fields,
    };

    // A name the language cannot spell is never invented: the inspector and
    // the scene file would have to agree with it, and have none to offer.
    if let Some((field, reason)) = fields
        .iter()
        .find_map(|field| unspellable(field.name).map(|reason| (field, reason)))
    {
        return not_mirrored(name, &format!("its field `{}` {reason}", field.name));
    }

    // What a script may do with it: write it, when an author may — the rule
    // the editor's "+ Add Component" follows — or only hold it as a value.
    let (keyword, why) = writable(registration);
    let preface = why.map_or(String::new(), |why| format!("// {name}: {why}\n"));

    // A marker carries no data, and one line says so without looking like a
    // declaration whose body went missing.
    if fields.is_empty() {
        return format!("{preface}{keyword} {name} {{ }}\n");
    }

    // What each field is to a script comes from the field type itself
    // (`ScriptField`), never from how its type happens to be spelled.
    let types: Vec<Option<String>> = (0..fields.len())
        .map(|slot| (registration.script_type)(slot).map(|ty| ty.to_string()))
        .collect();

    // With no field expressible, an empty struct would read as a marker.
    if types.iter().all(Option::is_none) {
        return not_mirrored(name, "Ergon cannot yet name the type of any of its fields");
    }

    let mut declaration = format!("{preface}{keyword} {name} {{\n");
    for (field, ty) in fields.iter().zip(types) {
        match ty {
            Some(ty) => declaration.push_str(&format!("    {ty} {};\n", field.name)),
            None => declaration.push_str(&format!(
                "    // {}: no Ergon type for `{}` yet.\n",
                field.name, field.ty
            )),
        }
    }
    declaration.push_str("}\n");
    declaration
}

/// How a script may hold `registration`: as a `component` it writes, or as a
/// `struct` value only — with the reason, which the module says above it.
fn writable(registration: &ComponentRegistration) -> (&'static str, Option<String>) {
    let (placed, natives) = khora_script::native::world::PLACEMENT;
    if registration.type_name == placed {
        return (
            "struct",
            Some(format!(
                "not written as a component — placed with {}",
                natives
                    .iter()
                    .map(|native| format!("`{native}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        );
    }
    match registration.provenance {
        provenance if provenance.is_hand_authorable() => ("component", None),
        ComponentProvenance::ToolAuthored => (
            "struct",
            Some("not written by a script — set by its own operations".to_owned()),
        ),
        _ => (
            "struct",
            Some("written by the engine, not by a script".to_owned()),
        ),
    }
}

/// A component that has no declaration, and the reason.
fn not_mirrored(name: &str, reason: &str) -> String {
    format!("// {name} — not mirrored: {reason}.\n")
}

/// Returns why an Ergon source could not spell `name` as a field, if it could not.
///
/// Asked of the lexer, so the keyword list has one home. A tuple index and a
/// reserved word (`Script.behavior`) get different reasons: only the second
/// could be fixed by renaming the Rust field.
fn unspellable(name: &str) -> Option<&'static str> {
    match khora_script::lex(name)
        .tokens
        .first()
        .map(|token| &token.kind)
    {
        Some(TokenKind::Ident(spelled)) if spelled == name => None,
        Some(TokenKind::Keyword(_)) => Some("is a word Ergon reserves"),
        _ => Some("is not a name Ergon can spell"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_script::compile_module;
    use khora_script::MemoryLoader;

    /// Compiles `source` as `main.erg`, with the mirrors available to import.
    fn compile(source: &str) -> khora_script::CompileOutcome {
        let loader = PreludeLoader::new(MemoryLoader::new().with("main.erg", source));
        compile_module(&loader, "main.erg")
    }

    /// **The whole module type-checks.**
    ///
    /// Every declaration the generator emits is put in front of the real
    /// checker, so a type spelling it invents that the language does not know —
    /// or a field name that does not parse — fails here rather than in whichever
    /// script first imported it.
    #[test]
    fn the_mirrors_compile() {
        let result = compile("import \"engine/components.erg\";\nfn void Main() { }");

        assert!(
            result.diagnostics.is_empty(),
            "the generated mirrors do not compile: {:?}\n\n{}",
            result.diagnostics,
            mirror_source()
        );
    }

    /// Whether a diagnostic is the bytecode's missing ECS bridge rather than
    /// anything to do with the mirrors.
    fn is_lowering_limit(diagnostic: &khora_script::Diagnostic) -> bool {
        diagnostic
            .message
            .contains("only an engine type's components can be read yet")
    }

    /// **A component's field resolves through the mirror, with its type.**
    ///
    /// `.y` is the proof of the type rather than decoration: it resolves only
    /// because `translation` came back as an engine type that declares a `y`. A
    /// mirror that gave it `int` would fail here instead.
    #[test]
    fn a_component_field_resolves_with_its_type() {
        let result = compile(
            "import \"engine/components.erg\";
             fn float Height(Transform t) { return t.translation.y; }",
        );

        let unexpected: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| !is_lowering_limit(d))
            .collect();
        assert!(
            unexpected.is_empty(),
            "the checker should be satisfied; got {unexpected:?}"
        );
    }

    /// And the other half: a misspelling is refused, and the report points at
    /// the access that is wrong rather than at the statement or the file.
    ///
    /// The span covers the whole field access — `Expr::Field` carries one span,
    /// not one per part — so it isolates the mistake without singling out the
    /// identifier.
    #[test]
    fn a_misspelled_field_is_refused_where_it_was_written() {
        let source = "import \"engine/components.erg\";
             fn float Height(Transform t) { return t.tranlsation.y; }";
        let result = compile(source);

        assert!(!result.succeeded());
        let diagnostic = result
            .diagnostics
            .iter()
            .find(|d| d.message.contains("`Transform` has no field `tranlsation`"))
            .unwrap_or_else(|| panic!("got {:?}", result.diagnostics));

        let span = diagnostic.span;
        assert_eq!(
            &source[span.start as usize..span.end as usize],
            "t.tranlsation"
        );
    }

    /// **A mirrored component is a struct a script can read.**
    ///
    /// A mirror is an ordinary Ergon `struct`, so a value of it — here written
    /// as a literal, every field given — is built, passed and read like any
    /// other: the program compiles, and the read returns the field that was
    /// written.
    ///
    /// `.y` goes through two hops on purpose: `translation` is a struct field
    /// holding an engine type, and `y` is that engine type's own component, so
    /// both kinds of read are exercised on a mirrored value.
    #[test]
    fn a_mirrored_component_is_a_struct_a_script_can_read() {
        use khora_script::{Host, Machine, Run, Value};

        let result = compile(
            "import \"engine/components.erg\";
             fn float Height(Transform t) { return t.translation.y; }
             fn float Main() {
                 var t = Transform {
                     translation: Vec3(1.0, 2.5, 3.0),
                     rotation: Quat(0.0, 0.0, 0.0, 1.0),
                     scale: Vec3(1.0, 1.0, 1.0)
                 };
                 return Height(t);
             }",
        );
        assert!(
            result.diagnostics.is_empty(),
            "a struct read over a mirror should compile; got {:?}",
            result.diagnostics
        );
        let program = result.program.expect("a program with no diagnostics");

        let mut machine = Machine::new(&program, "Main", &[]).expect("`Main` is a function");
        let mut host = Host::new();
        let (run, _) = machine.run_counting(&program, &mut host, u64::MAX);
        assert!(matches!(run, Run::Completed), "got {run:?}");
        assert_eq!(machine.result(), Value::Float(2.5));
    }

    /// **A component is read from an entity with `Get`.**
    ///
    /// Against the real mirror, `e.Get(RigidBody)` compiles, the program says it
    /// reads `RigidBody`, and the copy it returns imports as the mirror's
    /// struct: the read returns the entity's mass. `e.RigidBody` stays refused —
    /// an entity has no fields — and the refusal names `Get`, the way to read
    /// one.
    #[test]
    fn a_component_is_read_from_an_entity_with_get() {
        use std::collections::HashMap;
        use std::sync::Arc;

        use khora_core::ecs::entity::EntityId;
        use khora_core::math::Vec3;
        use khora_core::script::{
            ColumnSnapshot, ComponentName, ComponentProjection, FrameComponents,
            ProjectedComponent, ScriptValue,
        };
        use khora_script::{Host, Machine, Run, Value};

        struct Rows(Vec<ScriptValue>);
        impl ColumnSnapshot for Rows {
            fn len(&self) -> usize {
                self.0.len()
            }
            fn read(&self, row: usize) -> ScriptValue {
                self.0[row].clone()
            }
        }

        let result = compile(
            "import \"engine/components.erg\";
             fn float Mass(Entity e) {
                 if (var b = e.Get(RigidBody)) { return b.mass; }
                 return -1.0;
             }",
        );
        assert!(
            result.succeeded(),
            "a read over the mirror should compile; got {:?}",
            result.diagnostics
        );
        let program = result.program.expect("a program with no errors");
        assert_eq!(program.reads, vec!["RigidBody".to_owned()]);

        let holder = EntityId {
            index: 3,
            generation: 1,
        };
        let row = ScriptValue::Struct(vec![
            ("mass".to_owned(), ScriptValue::Float(3.5)),
            ("ccd_enabled".to_owned(), ScriptValue::Bool(false)),
            ("initial_velocity".to_owned(), ScriptValue::Vec3(Vec3::ZERO)),
            (
                "initial_angular_velocity".to_owned(),
                ScriptValue::Vec3(Vec3::ZERO),
            ),
        ]);
        let projection = ComponentProjection::new(vec![ProjectedComponent::new(
            ComponentName::new("RigidBody"),
            vec![Box::new(Rows(vec![row]))],
            HashMap::from([(holder, (0, 0))]),
            None,
        )]);
        let mut host = Host::new();
        host.components = FrameComponents::new(Arc::new(projection));
        let mut machine =
            Machine::new(&program, "Mass", &[Value::Entity(holder)]).expect("`Mass` exists");
        let (run, _) = machine.run_counting(&program, &mut host, u64::MAX);
        assert!(matches!(run, Run::Completed), "got {run:?}");
        assert_eq!(machine.result(), Value::Float(3.5));

        let refused = compile(
            "import \"engine/components.erg\";
             fn float Mass(Entity e) { return e.RigidBody.mass; }",
        );
        assert!(!refused.succeeded());
        let refusal = refused
            .diagnostics
            .iter()
            .find(|d| d.message.contains("an entity has no field `RigidBody`"))
            .unwrap_or_else(|| panic!("got {:?}", refused.diagnostics));
        let note = refusal.note.as_deref().unwrap_or_default();
        assert!(
            note.contains("`Get`"),
            "the refusal should name `Get`; got note {note:?}"
        );
        assert!(
            !note.contains("not available"),
            "reading a component is available now; got note {note:?}"
        );
    }

    /// A type Ergon has no spelling for is named in a comment, not dropped.
    /// `Camera.projection` is an enum; the fields around it still work.
    #[test]
    fn an_unexpressible_field_is_named_rather_than_dropped() {
        let source = mirror_source();

        assert!(
            source.contains("// projection: no Ergon type for `ProjectionType` yet."),
            "got:\n{source}"
        );
        assert!(source.contains("float z_near;"));
    }

    /// A camera's aspect is the shape of the image it fills, not a field of the
    /// camera.
    ///
    /// The mirror offers scripts no `aspect_ratio` to read or write.
    #[test]
    fn the_camera_mirror_has_no_aspect_ratio() {
        let source = mirror_source();
        assert!(source.contains("float z_far;"), "got:\n{source}");
        assert!(!source.contains("aspect_ratio"), "got:\n{source}");
    }

    /// An enum component says it is not mirrored, rather than appearing as a
    /// component with no fields — which would be a different, false claim.
    #[test]
    fn a_one_of_component_says_why_it_is_absent() {
        assert!(mirror_source().contains("// MaterialRef — not mirrored: a one-of"));
    }

    /// A tuple struct is absent for a stated reason rather than mirrored under
    /// an invented field name.
    #[test]
    fn a_positional_component_says_why_it_is_absent() {
        assert!(
            mirror_source()
                .contains("// Name — not mirrored: its field `0` is not a name Ergon can spell."),
            "got:\n{}",
            mirror_source()
        );
    }

    /// A reserved word is its own reason, not folded into the positional one.
    ///
    /// `UiInteraction.state` has a perfectly good name that this language happens
    /// to have taken, which is a different problem with a different fix.
    #[test]
    fn a_reserved_field_name_is_reported_as_one() {
        assert!(
            mirror_source().contains(
                "// UiInteraction — not mirrored: its field `state` is a word Ergon reserves."
            ),
            "got:\n{}",
            mirror_source()
        );
    }

    /// **A marker and a component nothing can express are not the same answer.**
    ///
    /// `UiImage` has a field; it is an `AssetUUID`. Mirroring it as an empty
    /// struct would say it carries no data — the same conflation
    /// `ComponentShape::Opaque` exists to prevent, one layer up.
    #[test]
    fn a_component_with_nothing_expressible_is_not_mirrored_as_a_marker() {
        let source = mirror_source();

        assert!(
            source.contains("// UiImage — not mirrored: Ergon cannot yet name the type"),
            "got:\n{source}"
        );
        assert!(
            !source.contains("struct UiImage"),
            "a component with a field must not appear as a marker"
        );
    }

    /// And a real marker is declared, on one line.
    #[test]
    fn a_marker_is_declared_empty() {
        assert!(mirror_source().contains("struct Teleported { }"));
    }

    /// **Nothing is silently missing.**
    ///
    /// Every registered component appears — as a declaration or as a stated
    /// reason. Without this, a component whose fields all became unexpressible
    /// would quietly vanish from the language.
    #[test]
    fn every_component_is_either_declared_or_explained() {
        let source = mirror_source();

        for registration in inventory::iter::<ComponentRegistration> {
            let name = registration.type_name;
            assert!(
                source.contains(&format!("struct {name} {{"))
                    || source.contains(&format!("component {name} {{"))
                    || source.contains(&format!("// {name} — not mirrored")),
                "{name} is neither mirrored nor explained"
            );
        }
    }

    /// The mirror answers before the disk.
    ///
    /// A file left at that path is a copy of an older engine, and letting it win
    /// would put the author's tooling and the running engine into quiet
    /// disagreement.
    #[test]
    fn a_file_at_the_mirror_path_does_not_shadow_the_mirror() {
        let loader = PreludeLoader::new(
            MemoryLoader::new().with(MIRROR_MODULE, "struct Transform { int wrong; }"),
        );

        let served = loader.load(MIRROR_MODULE).expect("the mirror");
        assert!(served.contains("Vec3 translation;"));
        assert!(!served.contains("int wrong;"));
    }

    /// Everything else goes through untouched.
    #[test]
    fn every_other_module_comes_from_the_inner_loader() {
        let loader = PreludeLoader::new(MemoryLoader::new().with("a.erg", "fn void A() { }"));

        assert_eq!(loader.load("a.erg").as_deref(), Some("fn void A() { }"));
        assert_eq!(loader.load("absent.erg"), None);
    }

    /// Sorted, so the readable copy an IDE keeps does not reshuffle between
    /// builds and turn every diff into noise.
    #[test]
    fn the_declarations_are_in_name_order() {
        let names: Vec<&str> = mirror_source()
            .lines()
            .filter_map(|line| {
                line.strip_prefix("struct ")
                    .or_else(|| line.strip_prefix("component "))
            })
            .filter_map(|rest| rest.split_whitespace().next())
            .collect();

        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
    }

    /// The line declaring `name`, as a `component` or a `struct`, with the
    /// line before it.
    fn declared(source: &str, name: &str) -> Option<(&'static str, Option<String>)> {
        let lines: Vec<&str> = source.lines().collect();
        lines.iter().enumerate().find_map(|(at, line)| {
            let kind = if line.starts_with(&format!("component {name} {{")) {
                "component"
            } else if line.starts_with(&format!("struct {name} {{")) {
                "struct"
            } else {
                return None;
            };
            let before = at
                .checked_sub(1)
                .and_then(|previous| lines.get(previous))
                .map(|line| (*line).to_owned());
            Some((kind, before))
        })
    }

    /// **A script may write what an author may write.**
    ///
    /// Every component the editor's "+ Add Component" offers is declared a
    /// `component` — the physics and audio ones a game reaches for first among
    /// them.
    #[test]
    fn the_mirror_emits_components_not_structs() {
        let source = mirror_source();
        for name in ["RigidBody", "Collider", "AudioSource"] {
            assert_eq!(
                declared(source, name).map(|(kind, _)| kind),
                Some("component"),
                "`{name}` should be a component; got:\n{source}"
            );
        }

        for registration in inventory::iter::<ComponentRegistration> {
            let name = registration.type_name;
            let Some((kind, _)) = declared(source, name) else {
                continue; // Not mirrored, for a stated reason.
            };
            if registration.provenance.is_hand_authorable() && name != "Transform" {
                assert_eq!(kind, "component", "`{name}` is authorable; got:\n{source}");
            }
        }
    }

    /// **And nothing else is.**
    ///
    /// What the engine writes — `Derived`, `Runtime` — what a tool writes, and
    /// `Transform`, which has its own placement natives, stay plain structs,
    /// each saying why just above it.
    #[test]
    fn the_mirror_follows_the_add_component_rule() {
        let source = mirror_source();

        for registration in inventory::iter::<ComponentRegistration> {
            let name = registration.type_name;
            if registration.provenance.is_hand_authorable() && name != "Transform" {
                continue;
            }
            let Some((kind, before)) = declared(source, name) else {
                continue; // Not mirrored, for a stated reason.
            };
            assert_eq!(
                kind, "struct",
                "`{name}` must not be writable; got:\n{source}"
            );
            assert!(
                before.as_deref().is_some_and(|line| line.trim_start().starts_with("//")),
                "`{name}` should be preceded by a comment saying why it is not writable; got:\n{source}"
            );
        }

        let (kind, before) = declared(source, "Transform").expect("Transform is mirrored");
        assert_eq!(kind, "struct");
        let before = before.unwrap_or_default();
        assert!(
            before.contains("SetPosition"),
            "the comment above `Transform` names its placement natives; got {before:?}"
        );

        for engine_written in ["BodyMotion", "Teleported"] {
            let (kind, before) = declared(source, engine_written)
                .unwrap_or_else(|| panic!("`{engine_written}` is mirrored; got:\n{source}"));
            assert_eq!(kind, "struct");
            assert!(
                before
                    .as_deref()
                    .unwrap_or_default()
                    .contains("written by the engine"),
                "the comment above `{engine_written}` says the engine writes it; got {before:?}"
            );
        }
    }

    // ── The type mapping ──────────────────────────────
    //
    // `ScriptField` (`khora_core::script::field`) answers what a Rust field
    // type is to a script; the registration carries it here by slot.

    /// **The type, not its spelling.**
    ///
    /// `length` is spelled `Meters` and `stops` `Stops` — aliases a spelling
    /// cannot see through — and the registration types them `float` and
    /// `Entity[]`: that is what is mirrored. `odometer` is spelled `f32` but
    /// the registration gives it no script type, so it is a comment, whatever
    /// its spelling suggests.
    #[test]
    fn the_mirror_reads_the_trait_not_the_spelling() {
        use khora_core::script::ErgonType;
        use khora_data::scene::FieldSchema;

        struct Ruler;
        const FIELDS: &[FieldSchema] = &[
            FieldSchema {
                name: "length",
                ty: "Meters",
            },
            FieldSchema {
                name: "stops",
                ty: "Stops",
            },
            FieldSchema {
                name: "odometer",
                ty: "f32",
            },
        ];
        let registration = ComponentRegistration {
            type_id: std::any::TypeId::of::<Ruler>(),
            type_name: "Ruler",
            shape: ComponentShape::Fields(FIELDS),
            provenance: ComponentProvenance::Authored,
            formerly: &[],
            resumable: false,
            column_to_record: |_, _, _| unreachable!("the mirror reads no column"),
            stage: |_, _| unreachable!("the mirror stages nothing"),
            schema: || unreachable!("the mirror traces no schema"),
            schema_complete: || unreachable!("the mirror traces no schema"),
            column_to_snapshot: |_, _, _, _| unreachable!("the mirror reads no column"),
            stage_snapshot: |_, _| unreachable!("the mirror stages nothing"),
            create_default: |_, _| unreachable!("the mirror adds nothing"),
            to_json: |_, _| unreachable!("the mirror reads no world"),
            from_json: |_, _, _| unreachable!("the mirror writes no world"),
            remove: |_, _| unreachable!("the mirror writes no world"),
            script_type: |slot| match slot {
                0 => Some(ErgonType::Float),
                1 => Some(ErgonType::Array(Box::new(ErgonType::Entity))),
                _ => None,
            },
            script_default: || None,
            check: None,
        };

        let mirrored = declaration(&registration);
        assert!(
            mirrored.contains(
                "    float length;
"
            ),
            "got:
{mirrored}"
        );
        assert!(
            mirrored.contains(
                "    Entity[] stops;
"
            ),
            "got:
{mirrored}"
        );
        assert!(
            mirrored.contains(
                "    // odometer: no Ergon type for `f32` yet.
"
            ),
            "got:
{mirrored}"
        );
        assert!(
            !mirrored.contains("float odometer"),
            "got:
{mirrored}"
        );
    }

    /// The keyword list lives in the lexer, and this is how it is consulted
    /// rather than copied.
    #[test]
    fn a_reserved_word_is_told_apart_from_a_non_name() {
        assert_eq!(unspellable("translation"), None);
        assert_eq!(unspellable("state"), Some("is a word Ergon reserves"));
        assert_eq!(unspellable("behavior"), Some("is a word Ergon reserves"));
        assert_eq!(unspellable("0"), Some("is not a name Ergon can spell"));
        assert_eq!(
            unspellable("two words"),
            Some("is not a name Ergon can spell")
        );
        assert_eq!(unspellable(""), Some("is not a name Ergon can spell"));
    }

    // ── The engine's names ────────────

    /// **Every engine component's name, mirrored or not.**
    ///
    /// A script may not declare a component the engine has — and the mirror
    /// leaves some out (`UiInteraction`, whose field is a reserved word), so
    /// the list is the registry's, `formerly` names included, never the
    /// mirror's.
    #[test]
    fn the_prelude_names_every_engine_component() {
        let loader = PreludeLoader::new(MemoryLoader::new());
        let names = loader.engine_components();

        for registration in inventory::iter::<ComponentRegistration> {
            assert!(
                names.iter().any(|name| name == registration.type_name),
                "`{}` is missing from the engine's names",
                registration.type_name
            );
            for formerly in registration.formerly {
                assert!(
                    names.iter().any(|name| name == formerly),
                    "`{formerly}`, formerly `{}`, is missing",
                    registration.type_name
                );
            }
        }
        assert!(
            names.iter().any(|name| name == "UiInteraction"),
            "an unmirrored component is named too"
        );
        assert!(
            !names.iter().any(|name| name == "Vec3"),
            "a type is not a component"
        );
    }

    /// **The clash, against the real engine.**
    ///
    /// A script declaring `RigidBody` — or `UiInteraction`, which the mirror
    /// does not declare — is refused at the name, saying how to use the
    /// engine's instead.
    #[test]
    fn a_script_component_named_like_an_engine_one_is_refused() {
        for name in ["RigidBody", "UiInteraction"] {
            let source = format!("fn void Probe() {{ }}\ncomponent {name} {{ int n = 0; }}");
            let result = compile(&source);

            assert!(!result.succeeded(), "`{name}` should be refused");
            let refusal = result
                .diagnostics
                .iter()
                .find(|d| {
                    format!("{} {}", d.message, d.note.as_deref().unwrap_or_default())
                        .contains(&format!("`{name}` is an engine component"))
                })
                .unwrap_or_else(|| {
                    panic!(
                        "expected the engine-name refusal; got {:?}",
                        result.diagnostics
                    )
                });
            assert_eq!(
                &source[refusal.span.start as usize..refusal.span.end as usize],
                name
            );
        }

        let own = compile("component Shield { int charge = 3; }\nfn void Probe() { }");
        assert!(
            own.succeeded(),
            "a name the engine does not use compiles: {:?}",
            own.diagnostics
        );
    }

    // A component retired for these tests only: no registration has its
    // name, so only the retired list can make it the engine's.
    inventory::submit! {
        khora_data::scene::RetiredComponent { name: "Sundial" }
    }

    /// Asserts `source` is refused at `name`, as a name the engine's
    /// components hold.
    fn assert_refused_at_name(source: &str, name: &str) {
        let result = compile(source);
        assert!(!result.succeeded(), "`{name}` should be refused: {source}");
        let refusal = result
            .diagnostics
            .iter()
            .find(|d| {
                &source[d.span.start as usize..d.span.end as usize] == name
                    && format!("{} {}", d.message, d.note.as_deref().unwrap_or_default())
                        .contains("engine component")
            })
            .unwrap_or_else(|| {
                panic!(
                    "expected the engine-name refusal at `{name}`; got {:?}",
                    result.diagnostics
                )
            });
        assert!(
            refusal.message.contains(&format!("`{name}`")),
            "the refusal names `{name}`: {}",
            refusal.message
        );
    }

    /// **A retired name is still the engine's.**
    ///
    /// A load checks a file's declared names before the retired ones, so a
    /// script declaring a retired component's name would capture an old file's
    /// cells: the name is among the engine's, and a `component` or `behavior`
    /// declaring it is refused at the name.
    #[test]
    fn a_retired_component_name_is_refused_as_an_engine_one() {
        let loader = PreludeLoader::new(MemoryLoader::new());
        assert!(
            loader
                .engine_components()
                .iter()
                .any(|name| name == "Sundial"),
            "a retired name is among the engine's names"
        );
        for kind in ["component", "behavior"] {
            let source = format!("fn void Probe() {{ }}\n{kind} Sundial {{ int n = 0; }}");
            assert_refused_at_name(&source, "Sundial");
        }
    }

    /// **What a behavior replaced cannot be declared.**
    ///
    /// `"Script"` and `"ScriptState"` — the components a behavior replaced,
    /// retired — are refused as a `behavior` and as a `component`, at the name.
    #[test]
    fn a_script_may_not_declare_the_components_a_behavior_replaced() {
        for name in ["Script", "ScriptState"] {
            for kind in ["behavior", "component"] {
                let source = format!("fn void Probe() {{ }}\n{kind} {name} {{ int n = 0; }}");
                assert_refused_at_name(&source, name);
            }
        }
    }
}

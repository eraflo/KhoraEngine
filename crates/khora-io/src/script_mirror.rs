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

//! The engine's components, as Ergon declarations.
//!
//! Every component publishes a [`ComponentShape`]. This turns that shape into
//! Ergon source, so a script can name `Transform.translation` and be told at
//! compile time when it writes `tranlsation` instead.
//!
//! # Served, not written
//!
//! The mirrors are a [`SourceLoader`] answer, not a file. A generated file that
//! an author can open becomes stale and then edited, in that order, and the
//! second is discovered long after the first. Serving them in memory means the
//! mirror always describes *the engine that is running* — there is no older copy
//! for it to disagree with, and [`PreludeLoader`] deliberately answers before
//! the disk so a file that somehow exists at that path cannot shadow it.
//!
//! # Partial by design
//!
//! Ergon cannot yet name every type a component field can have. Such a field is
//! **kept as a comment** rather than dropped: the omission then reads where an
//! author looks for it, and the comment names the Rust type it could not
//! express — which is what [`FieldSchema::ty`] carries the source spelling for.
//!
//! Dropping the whole component instead would be worse: `RigidBody.mass` is
//! perfectly expressible, and losing it because `body_type` is an enum would
//! cost the author the fields that do work.
//!
//! # What a mirror buys today, and what it does not
//!
//! It buys the **checker**: a name resolves, a field resolves with its type, and
//! a misspelling is refused where it was written. It does not yet buy a *read* —
//! the compiler lowers `v.x` on an engine type and nothing else, so
//! `t.translation` type-checks and then stops at "only an engine type's
//! components can be read yet". That is a limit of the bytecode, older than this
//! module and pinned by a test here, and it is a compile error rather than a
//! wrong value at run time.
//!
//! The distinction matters because `ENGINE_TYPES` in `khora-script` records the
//! mistake this resembles: it once named `Transform`, which had no fields at
//! all, so `Transform t;` shaped cleanly and failed with the misleading
//! "`Transform` has no `x`". A mirror is the opposite — the fields are real and
//! the remaining error names the thing that is actually missing.
//!
//! [`ComponentShape`]: khora_data::scene::ComponentShape
//! [`FieldSchema::ty`]: khora_data::scene::FieldSchema::ty

use std::sync::OnceLock;

use khora_data::scene::{ComponentRegistration, ComponentShape};
use khora_script::modules::SourceLoader;
use khora_script::TokenKind;

/// Where a script imports the mirrors from.
///
/// `import "engine/components.erg";` — an ordinary import, so a script that does
/// not use a component pays nothing, and a project declaring its own `Camera`
/// only meets the clash if it asked for ours.
pub const MIRROR_MODULE: &str = "engine/components.erg";

/// The Ergon source mirroring every registered component.
///
/// Built once. The component registry is filled by `inventory` at link time, so
/// the answer cannot change while the process runs.
pub fn mirror_source() -> &'static str {
    static SOURCE: OnceLock<String> = OnceLock::new();
    SOURCE.get_or_init(generate)
}

/// A loader that answers [`MIRROR_MODULE`] itself and delegates everything else.
///
/// Stacked over whatever serves a project's own scripts:
///
/// ```no_run
/// use khora_io::script_compile::{compile_module, DiskLoader};
/// use khora_io::script_mirror::PreludeLoader;
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

    /// The wrapped loader.
    pub fn inner(&self) -> &L {
        &self.inner
    }
}

impl<L: SourceLoader> SourceLoader for PreludeLoader<L> {
    fn load(&self, path: &str) -> Option<String> {
        if path == MIRROR_MODULE {
            // Ahead of the inner loader on purpose: a file left at this path is
            // a copy of an older engine, and letting it win would put the
            // author's tools and the running engine into quiet disagreement.
            return Some(mirror_source().to_owned());
        }
        self.inner.load(path)
    }
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

    // A name the language cannot spell — a tuple index, or a word it reserves.
    // Mirroring one means inventing a name, and that name would then have to
    // agree with the inspector and with what a scene file writes, neither of
    // which has one to offer. Better absent than invented.
    if let Some((field, reason)) = fields
        .iter()
        .find_map(|field| unspellable(field.name).map(|reason| (field, reason)))
    {
        return not_mirrored(name, &format!("its field `{}` {reason}", field.name));
    }

    // A marker carries no data, and one line says so without looking like a
    // declaration whose body went missing.
    if fields.is_empty() {
        return format!("struct {name} {{ }}\n");
    }

    let types: Vec<Option<String>> = fields.iter().map(|field| ergon_type(field.ty)).collect();

    // A component none of whose fields can be expressed would otherwise mirror
    // as an empty struct — which reads as a marker. That is the same conflation
    // `ComponentShape::Opaque` exists to prevent, one layer up.
    if types.iter().all(Option::is_none) {
        return not_mirrored(name, "Ergon cannot yet name the type of any of its fields");
    }

    let mut declaration = format!("struct {name} {{\n");
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

/// A component that has no declaration, and the reason.
fn not_mirrored(name: &str, reason: &str) -> String {
    format!("// {name} — not mirrored: {reason}.\n")
}

/// The Ergon spelling of a Rust type, when there is one.
///
/// Deliberately refuses the widths that would not survive the trip: Ergon's
/// `int` is 64-bit signed and its `float` is 32-bit, so `u64` and `f64` are left
/// unexpressed rather than silently truncated. A field that reads back a
/// different number than it was given is worse than a field a script cannot see.
fn ergon_type(rust: &str) -> Option<String> {
    let rust = rust.trim();

    if let Some(inner) = generic_argument(rust, "Option") {
        return ergon_type(inner).map(|inner| format!("{inner}?"));
    }
    if let Some(inner) = generic_argument(rust, "Vec") {
        return ergon_type(inner).map(|inner| format!("{inner}[]"));
    }

    Some(
        match rust {
            "f32" => "float",
            "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" => "int",
            "bool" => "bool",
            "String" => "string",
            "EntityId" => "Entity",
            "Quaternion" => "Quat",
            "LinearRgba" => "Color",
            "Vec2" | "Vec3" | "Vec4" => rust,
            _ => return None,
        }
        .to_owned(),
    )
}

/// The `T` of `Name<T>`, when `rust` is exactly that.
///
/// The schema carries what `stringify!` produced, so the separators are spaced —
/// `Vec < EntityId >`. Matching the head exactly is what keeps `Vec3` from being
/// read as a `Vec` of something.
fn generic_argument<'a>(rust: &'a str, name: &str) -> Option<&'a str> {
    let (head, rest) = rust.split_once('<')?;
    if head.trim() != name {
        return None;
    }
    Some(rest.trim_end().strip_suffix('>')?.trim())
}

/// Why an Ergon source could not spell `name` as a field, if it could not.
///
/// Asked of the lexer rather than answered here, so the keyword list has one
/// home. `Script.behavior` and `UiInteraction.state` are both reserved words:
/// mirroring either would produce a module that does not parse, and the mistake
/// would then surface in every script that imported it rather than here.
///
/// The two reasons are kept apart because they are different problems. A tuple
/// index will never have a name; a reserved word has one that a future version
/// of the language could let through, or that renaming the Rust field would fix.
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
    use crate::script_compile::compile_module;
    use khora_script::MemoryLoader;

    /// Compiles `source` as `main.erg`, with the mirrors available to import.
    fn compile(source: &str) -> crate::script_compile::Compiled {
        let loader = PreludeLoader::new(MemoryLoader::new().with("main.erg", source));
        compile_module(&loader, "main.erg")
    }

    /// **The whole module type-checks.** Every declaration the generator emits
    /// is put in front of the real checker, so a type spelling it invents that
    /// the language does not know — or a field name that does not parse — fails
    /// here rather than in whichever script first imported it.
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

    /// **What P1 exists to prove.** A component's field resolves through the
    /// mirror, with its type.
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

    /// **The limit, pinned.** The compiler lowers `v.x` on an engine type and
    /// nothing else, so a mirrored field type-checks and then stops here. It is
    /// older than this module — every Ergon `struct` has always had it — and it
    /// is a compile error rather than a wrong value at run time.
    ///
    /// When the projected read lands, this test is what says so.
    #[test]
    fn reading_a_mirrored_field_still_needs_the_ecs_bridge() {
        let result = compile(
            "import \"engine/components.erg\";
             fn float Height(Transform t) { return t.translation.y; }",
        );

        assert!(
            result.diagnostics.iter().any(is_lowering_limit),
            "got {:?}",
            result.diagnostics
        );
        assert!(!result.succeeded());
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

    /// A reserved word is its own reason, not folded into the positional one:
    /// `Script.behavior` has a perfectly good name that this language happens to
    /// have taken, which is a different problem with a different fix.
    #[test]
    fn a_reserved_field_name_is_reported_as_one() {
        assert!(
            mirror_source().contains(
                "// Script — not mirrored: its field `behavior` is a word Ergon reserves."
            ),
            "got:\n{}",
            mirror_source()
        );
    }

    /// **A marker and a component nothing can express are not the same answer.**
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

    /// **Nothing is silently missing.** Every registered component appears —
    /// as a declaration or as a stated reason. Without this, a component whose
    /// fields all became unexpressible would quietly vanish from the language.
    #[test]
    fn every_component_is_either_declared_or_explained() {
        let source = mirror_source();

        for registration in inventory::iter::<ComponentRegistration> {
            let name = registration.type_name;
            assert!(
                source.contains(&format!("struct {name} {{"))
                    || source.contains(&format!("// {name} — not mirrored")),
                "{name} is neither mirrored nor explained"
            );
        }
    }

    /// The mirror answers before the disk. A file left at that path is a copy of
    /// an older engine, and letting it win would put the author's tooling and
    /// the running engine into quiet disagreement.
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
            .filter_map(|line| line.strip_prefix("struct "))
            .filter_map(|rest| rest.split_whitespace().next())
            .collect();

        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
    }

    // ── The type mapping ──────────────────────────────

    #[test]
    fn the_primitives_map_to_their_ergon_spellings() {
        assert_eq!(ergon_type("f32").as_deref(), Some("float"));
        assert_eq!(ergon_type("i32").as_deref(), Some("int"));
        assert_eq!(ergon_type("bool").as_deref(), Some("bool"));
        assert_eq!(ergon_type("String").as_deref(), Some("string"));
        assert_eq!(ergon_type("EntityId").as_deref(), Some("Entity"));
        assert_eq!(ergon_type("Quaternion").as_deref(), Some("Quat"));
        assert_eq!(ergon_type("LinearRgba").as_deref(), Some("Color"));
    }

    /// **A number that would not survive the trip is refused.** Ergon's `int` is
    /// 64-bit signed and its `float` is 32-bit, so a `u64` read back as an `int`
    /// could differ from what it was given — and a field that lies is worse than
    /// one a script cannot see.
    #[test]
    fn a_width_that_would_truncate_is_left_unexpressed() {
        assert_eq!(ergon_type("u64"), None);
        assert_eq!(ergon_type("usize"), None);
        assert_eq!(ergon_type("f64"), None);
    }

    /// The schema carries what `stringify!` produced, spaces and all.
    #[test]
    fn the_spaced_generics_stringify_produces_are_understood() {
        assert_eq!(ergon_type("Vec < EntityId >").as_deref(), Some("Entity[]"));
        assert_eq!(ergon_type("Option < i32 >").as_deref(), Some("int?"));
        assert_eq!(
            ergon_type("Vec < Vec < Vec3 > >").as_deref(),
            Some("Vec3[][]")
        );
    }

    /// `Vec3` is a type, not a `Vec` of `3`. Matching the head exactly is what
    /// keeps the two apart.
    #[test]
    fn a_vec3_is_not_read_as_a_collection() {
        assert_eq!(ergon_type("Vec3").as_deref(), Some("Vec3"));
    }

    /// A wrapper whose contents cannot be expressed cannot be expressed either —
    /// there is no half-answer for `Vec<(String, ScriptValue)>`.
    #[test]
    fn a_generic_over_something_unexpressible_is_unexpressible() {
        assert_eq!(ergon_type("Vec < [u32; 2] >"), None);
        assert_eq!(ergon_type("Option < ScriptValue >"), None);
        assert_eq!(ergon_type("UiRect < f32 >"), None);
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
}

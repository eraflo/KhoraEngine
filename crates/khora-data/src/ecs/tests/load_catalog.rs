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

//! The load catalogue: what preparing a load reads of a world, frozen so a
//! load can be prepared off the frame.
//!
//! A catalogue answers the domain of every component the world knows and the
//! components declared while the engine runs, by name. It is an immutable
//! snapshot: a change of the world's components gives the world a new
//! catalogue of another generation, and leaves a catalogue taken before as
//! it was.

use std::any::TypeId;
use std::sync::Arc;

use khora_core::script::ScriptValue;

use super::runtime_components::{declare, field, layout};
use super::{Position, Velocity};
use crate::ecs::{
    ComponentKey, ComponentProvenance, FieldKind, PackedField, SemanticDomain, Transform, World,
};
use crate::scene::{ComponentRegistration, LoadCatalog};

/// `Health`'s fields: a counter and a label.
fn health() -> Vec<PackedField> {
    vec![
        field("current", FieldKind::Int, ScriptValue::Int(100)),
        field("label", FieldKind::Value, ScriptValue::Str(String::new())),
    ]
}

/// **A catalogue goes anywhere.** It is shared as an `Arc` and read on
/// another thread than the world's.
#[test]
fn the_load_catalogue_is_shared_across_threads() {
    fn shareable<T: Send + Sync + 'static>(_: &T) {}
    let mut world = World::new();
    declare(&mut world, "Health", SemanticDomain::Physics, health());
    let catalog: Arc<LoadCatalog> = world.load_catalog();
    shareable(&catalog);

    let generation = catalog.generation();
    let read = std::thread::scope(|scope| {
        let catalog = Arc::clone(&catalog);
        scope
            .spawn(move || {
                (
                    catalog.generation(),
                    catalog.declared("Health").map(|declared| declared.domain),
                )
            })
            .join()
            .expect("the reader panicked")
    });
    assert_eq!(read, (generation, Some(SemanticDomain::Physics)));
}

/// **The catalogue knows every domain the world does**: each registered
/// Rust component, in the domain the world keeps it in.
#[test]
fn the_load_catalogue_answers_the_domain_of_every_registered_component() {
    let world = World::new();
    let catalog = world.load_catalog();

    let mut checked = 0;
    for reg in inventory::iter::<ComponentRegistration> {
        assert_eq!(
            catalog.domain_of(ComponentKey::Rust(reg.type_id)),
            world.component_domain(reg.type_id),
            "the domain of `{}`",
            reg.type_name
        );
        checked += 1;
    }
    assert!(checked > 0, "no registration was checked");
    assert_eq!(
        catalog.domain_of(ComponentKey::of::<Transform>()),
        world.component_domain(TypeId::of::<Transform>())
    );
    assert_eq!(
        catalog.domain_of(ComponentKey::of::<Position>()),
        None,
        "a component the world never registered has no domain"
    );
}

/// **The catalogue names a declared component as the world keeps it**: its
/// key, domain, fields and whether a game save keeps it. A Rust
/// component's name, or a name nobody declared, is no declared component.
#[test]
fn the_load_catalogue_names_each_declared_component_as_the_world_keeps_it() {
    let mut world = World::new();
    let key = declare(&mut world, "Health", SemanticDomain::Physics, health());
    let catalog = world.load_catalog();

    let declared = catalog.declared("Health").expect("`Health` is declared");
    assert_eq!(declared.key, key);
    assert_eq!(declared.key, ComponentKey::named("Health"));
    assert_eq!(declared.domain, SemanticDomain::Physics);
    assert_eq!(declared.layout.fields(), layout(health()).fields());
    assert!(!declared.resumable);
    assert_eq!(catalog.domain_of(key), Some(SemanticDomain::Physics));

    assert!(catalog.declared("Transform").is_none(), "a Rust component");
    assert!(catalog.declared("Nobody").is_none(), "an undeclared name");
}

/// **An unchanged world keeps its catalogue's generation.** Spawning,
/// writing, despawning and ending frames change no component the catalogue
/// describes.
#[test]
fn an_unchanged_world_keeps_its_catalogue_generation() {
    let mut world = World::new();
    let key = declare(&mut world, "Health", SemanticDomain::Physics, health());
    let first = world.load_catalog().generation();
    assert_eq!(world.load_catalog().generation(), first, "asked twice");

    let a = world.spawn(Transform::identity());
    let b = world.spawn(Transform::identity());
    world
        .add_runtime_component(a, key, &ScriptValue::Unit)
        .expect("`Health` attaches");
    world.get_mut::<Transform>(b).expect("placed").translation.x = 4.0;
    assert!(world.despawn(b));
    world.reserver().reserve();
    world.settle_reservations();
    world.release_unspent();

    assert_eq!(
        world.load_catalog().generation(),
        first,
        "nothing the catalogue describes changed"
    );
}

/// **Registering a Rust component moves the generation**, and the new
/// catalogue knows its domain. A catalogue taken before is left as it was.
#[test]
fn registering_a_component_moves_the_catalogue_generation() {
    let mut world = World::new();
    let before = world.load_catalog();

    world.register_component::<Position>(SemanticDomain::Spatial);

    let after = world.load_catalog();
    assert_ne!(after.generation(), before.generation());
    assert_eq!(
        after.domain_of(ComponentKey::of::<Position>()),
        Some(SemanticDomain::Spatial)
    );
    assert_eq!(
        before.domain_of(ComponentKey::of::<Position>()),
        None,
        "a catalogue taken before the registration changed"
    );
}

/// **A registration that changes nothing keeps the generation**: a Rust
/// component registered again in its own domain, a declaration refused
/// because its name is taken.
#[test]
fn a_registration_that_changes_nothing_keeps_the_catalogue_generation() {
    let mut world = World::new();
    world.register_component::<Velocity>(SemanticDomain::Spatial);
    declare(&mut world, "Health", SemanticDomain::Physics, health());
    let generation = world.load_catalog().generation();

    world.register_component::<Velocity>(SemanticDomain::Spatial);
    assert!(
        world
            .register_runtime_component(super::runtime_components::decl(
                "Health",
                SemanticDomain::Script,
                health(),
            ))
            .is_err(),
        "a second `Health` is refused"
    );

    assert_eq!(world.load_catalog().generation(), generation);
    assert_eq!(
        world
            .load_catalog()
            .declared("Health")
            .map(|declared| declared.domain),
        Some(SemanticDomain::Physics)
    );
}

/// **Declaring a component moves the generation**, and the new catalogue
/// names it; the one taken before does not.
#[test]
fn declaring_a_component_moves_the_catalogue_generation() {
    let mut world = World::new();
    let before = world.load_catalog();

    declare(&mut world, "Health", SemanticDomain::Physics, health());

    let after = world.load_catalog();
    assert_ne!(after.generation(), before.generation());
    assert!(after.declared("Health").is_some());
    assert!(
        before.declared("Health").is_none(),
        "a catalogue taken before the declaration changed"
    );
}

/// **Moving a declared component to another domain moves the generation**,
/// and the new catalogue gives the new domain.
#[test]
fn moving_a_declared_component_moves_the_catalogue_generation() {
    let mut world = World::new();
    let key = declare(&mut world, "Health", SemanticDomain::Physics, health());
    let before = world.load_catalog();

    world
        .move_component_domain(key, SemanticDomain::Script)
        .expect("`Health` moves");

    let after = world.load_catalog();
    assert_ne!(after.generation(), before.generation());
    assert_eq!(
        after.declared("Health").map(|declared| declared.domain),
        Some(SemanticDomain::Script)
    );
    assert_eq!(after.domain_of(key), Some(SemanticDomain::Script));
    assert_eq!(
        before.declared("Health").map(|declared| declared.domain),
        Some(SemanticDomain::Physics),
        "a catalogue taken before the move changed"
    );
}

/// **Relaying a declared component out moves the generation**, and the new
/// catalogue gives the new fields.
#[test]
fn relaying_out_a_declared_component_moves_the_catalogue_generation() {
    let mut world = World::new();
    let key = declare(&mut world, "Health", SemanticDomain::Physics, health());
    let before = world.load_catalog();
    let mut grown = health();
    grown.push(field("max", FieldKind::Int, ScriptValue::Int(100)));

    world
        .relayout(key, layout(grown.clone()))
        .expect("`Health` relays out");

    let after = world.load_catalog();
    assert_ne!(after.generation(), before.generation());
    let fields = |catalog: &LoadCatalog| {
        catalog
            .declared("Health")
            .map(|declared| declared.layout.fields().to_vec())
    };
    assert_eq!(fields(&after), Some(layout(grown).fields().to_vec()));
    assert_eq!(
        fields(&before),
        Some(layout(health()).fields().to_vec()),
        "a catalogue taken before the relayout changed"
    );
}

/// **Giving a declared component another provenance moves the generation**:
/// whether a game save keeps it decides how a load reads it.
#[test]
fn changing_a_declared_components_provenance_moves_the_catalogue_generation() {
    let mut world = World::new();
    let key = declare(&mut world, "Health", SemanticDomain::Physics, health());
    let before = world.load_catalog();

    world
        .set_provenance(key, ComponentProvenance::Runtime, true)
        .expect("`Health` takes the provenance");

    let after = world.load_catalog();
    assert_ne!(after.generation(), before.generation());
    assert_eq!(
        after.declared("Health").map(|declared| declared.resumable),
        Some(true)
    );
}

/// **The catalogue names the world's nowhere entity**, the one a reference
/// to nothing loads as: the world has it from the start, so taking the
/// catalogue reserves nothing.
#[test]
fn the_load_catalogue_names_the_worlds_nowhere_entity() {
    let world = World::new();
    let nowhere = world
        .nowhere_entity()
        .expect("a world has its nowhere entity");

    assert_eq!(world.load_catalog().nowhere(), nowhere);
    assert_eq!(world.nowhere_entity(), Some(nowhere));
}

/// **A load that cannot land since the world changed says why.** Prepared
/// while `Position` was only the name of a component the file declares, the
/// load is committed after the world registered a Rust component of that
/// name: a load prepared now fails, naming the clash. The stale load adds
/// nothing, and what it gives back carries that failure — not the empty
/// report of a scene with nothing in it.
#[test]
fn a_stale_load_that_fails_now_reports_the_failure() {
    let mut src = World::new();
    let key = declare(&mut src, "Position", SemanticDomain::Spatial, health());
    let holder = src.spawn(Transform::identity());
    src.add_runtime_component(holder, key, &ScriptValue::Struct(Vec::new()))
        .expect("attaches");
    src.mark_authored(holder).expect("authored");
    let record = crate::scene::capture_world(&src).expect("captures");

    let mut world = World::new();
    let own = world.spawn(Transform::identity());
    let catalog = world.load_catalog();
    let stale = crate::scene::prepare_with(
        &catalog,
        world.hold_reservations(),
        Arc::new(record.clone()),
    )
    .expect("prepares while `Position` is the file's");
    world.register_component::<Position>(SemanticDomain::Spatial);

    let now = crate::scene::prepare(&mut world, &record)
        .err()
        .expect("a load prepared now fails: `Position` is a Rust component's name");
    let applied = stale.commit(&mut world, crate::scene::Identity::Keep);

    assert!(applied.entities.is_empty(), "the stale load added entities");
    assert_eq!(world.entity_count(), 1);
    assert!(world.contains(own));
    assert!(
        !applied.report.entries.is_empty(),
        "the stale load failed now ({}) yet reports nothing: indistinguishable from an empty \
         scene loaded",
        now.message
    );
}

/// **The generation watch moves with the registry.** A watch taken once,
/// before any change, reads the catalogue's generation as each change
/// moves it — a registration, a declaration, a relayout — and every watch
/// the world gives is that one counter. Nothing else moves it.
#[test]
fn the_generation_watch_moves_with_the_registry() {
    use std::sync::atomic::Ordering;

    let mut world = World::new();
    let watch = world.catalog_generation();
    let read = || watch.load(Ordering::SeqCst);
    let start = read();
    assert_eq!(start, world.load_catalog().generation());

    world.register_component::<Position>(SemanticDomain::Spatial);
    let registered = read();
    assert_ne!(registered, start, "a registration moves the watch");
    assert_eq!(registered, world.load_catalog().generation());

    let key = declare(&mut world, "Health", SemanticDomain::Physics, health());
    let declared = read();
    assert_ne!(declared, registered, "a declaration moves the watch");
    assert_eq!(declared, world.load_catalog().generation());

    let mut grown = health();
    grown.push(field("max", FieldKind::Int, ScriptValue::Int(100)));
    world
        .relayout(key, layout(grown))
        .expect("`Health` relays out");
    let relaid = read();
    assert_ne!(relaid, declared, "a relayout moves the watch");
    assert_eq!(relaid, world.load_catalog().generation());

    let a = world.spawn(Transform::identity());
    assert!(world.despawn(a));
    world.release_unspent();
    assert_eq!(read(), relaid, "spawning and frames move nothing");
    assert!(
        Arc::ptr_eq(&watch, &world.catalog_generation()),
        "every watch is the world's one counter"
    );
}

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

//! The registrations, made at link time through `inventory`, by which a scene
//! saves, loads and inspects every component with no per-type code.
//!
//! See the book: concepts/serialization.md, "How components serialize".

use std::any::TypeId;
use std::collections::HashMap;
use std::sync::OnceLock;

use khora_core::ecs::entity::EntityId;

pub(crate) use super::kept::Kept;
use super::record::{Record, RecordError, ReferenceReader, ReferenceWriter, ReportEntry};
pub use super::staged_value::StagedValue;
use crate::ecs::{AnyVec, ComponentProvenance, World};

/// Writes a column row positionally: see [`ComponentRegistration::column_to_snapshot`].
pub type ColumnToSnapshotFn =
    fn(&dyn AnyVec, usize, &mut Vec<u8>, &mut dyn ReferenceWriter) -> Result<(), RecordError>;

/// Tells whether a column row holds a value only this session has: see
/// [`ComponentRegistration::session_only`].
pub type SessionOnlyFn = fn(&dyn AnyVec, usize) -> bool;

/// Runs a component's own check over a world: see [`ComponentRegistration::check`].
pub type CheckFn = fn(&World) -> Vec<(EntityId, crate::ecs::FieldProblem)>;

/// The registration of a serializable component type, submitted by `#[derive(Component)]`.
///
/// Saving, loading and the inspector all go through the generated
/// `Serializable<Type>` mirror, so a field tagged `#[component(skip)]` is left
/// out of each.
pub struct ComponentRegistration {
    /// The `TypeId` of the component type.
    pub type_id: TypeId,

    /// What this component looks like from outside Rust: see
    /// [`ComponentShape`](crate::scene::ComponentShape).
    pub shape: crate::scene::ComponentShape,

    /// A human-readable name for the component (e.g., "Camera", "Light").
    pub type_name: &'static str,

    /// Who writes this component — see [`ComponentProvenance`].
    ///
    /// Read wherever the author's data must be told from the engine's, in place
    /// of a list of type names.
    pub provenance: ComponentProvenance,

    /// The names this component had before a rename (`formerly`), read as it.
    ///
    /// A save is always written under the current name.
    pub formerly: &'static [&'static str],

    /// Whether a game save keeps this runtime component so play resumes: `#[component(resumable)]`.
    pub resumable: bool,

    /// Writes row `row` of a page column as a record, entity references through `references`.
    ///
    /// The value is read whole and written by name, so the column's layout
    /// never reaches a file.
    pub column_to_record:
        fn(&dyn AnyVec, usize, &mut dyn ReferenceWriter) -> Result<Record, RecordError>,

    /// Reads the component back from a record, references through `references`.
    ///
    /// Touches no world: returns what the load would add, and what reading adapted.
    pub stage: fn(&Record, &mut dyn ReferenceReader) -> Result<StagedComponent, RecordError>,

    /// Returns the fingerprint of the schema a snapshot writes this component in.
    ///
    /// A snapshot is read only by the schema that wrote it.
    pub schema: fn() -> u64,

    /// Whether the schema was traced in full.
    ///
    /// A component whose schema was not is never written to, nor read from, a
    /// snapshot: no fingerprint tells its versions apart.
    pub schema_complete: fn() -> bool,

    /// Writes row `row` of a page column of this component positionally —
    /// no names, its references through `references` — appending to `out`.
    pub column_to_snapshot: ColumnToSnapshotFn,

    /// Reads the component back from the bytes a snapshot holds for it, which
    /// must be consumed exactly, its references through `references`.
    pub stage_snapshot: fn(&[u8], &mut dyn ReferenceReader) -> Result<StagedComponent, RecordError>,

    /// Adds a default instance of this component to the entity.
    pub create_default: fn(&mut World, EntityId) -> Result<(), String>,

    /// Returns a JSON view of the component on `entity`, `None` if the entity lacks it.
    ///
    /// Fields tagged `#[component(skip)]` are left out, as from a saved record.
    pub to_json: fn(&World, EntityId) -> Option<serde_json::Value>,

    /// Applies a JSON value back to the component on `entity`.
    ///
    /// Tolerates a partial update: the value is read against the full mirror.
    pub from_json: fn(&mut World, EntityId, &serde_json::Value) -> Result<(), String>,

    /// Removes this component alone from `entity`, leaving every other in place.
    pub remove: fn(&mut World, EntityId) -> Result<(), String>,

    /// The Ergon type of the field at a slot — the field's position in
    /// [`shape`](Self::shape) — `None` for a field a script cannot reach.
    pub script_type: fn(usize) -> Option<khora_core::script::ErgonType>,
    /// The component's default, as a script reads it — what a script's `Add`
    /// starts from, before its own fields. `None` for one a script cannot read.
    pub script_default: fn() -> Option<khora_core::script::ScriptValue>,
    /// The component's own check (`#[component(check = …)]`), `None` when it declares none.
    ///
    /// Run over a world, it returns each field the engine cannot use, with its
    /// entity. It never rewrites a value.
    pub check: Option<CheckFn>,
    /// Tells whether the value at a page row lives only in this session, `None`
    /// when every value of the component is saved.
    ///
    /// Such a value names what no file holds (a mesh made in code, kept only
    /// by the session's asset store): a scene or a game save leaves it out,
    /// saving the entity without it, and reports it
    /// [`ReportKind::NotSaved`](super::record::ReportKind::NotSaved). A
    /// snapshot, read back in the same session, keeps it.
    pub session_only: Option<SessionOnlyFn>,
}

inventory::collect!(ComponentRegistration);

impl ComponentRegistration {
    /// Returns whether a save holds this component: one an author or a tool wrote,
    /// not one the engine derives or writes while running.
    pub fn is_saved(&self) -> bool {
        self.provenance.is_copied_on_duplicate()
    }

    /// Returns whether the value at `row` of `column` lives only in this session.
    ///
    /// A scene or a game save leaves such a value out; see
    /// [`session_only`](Self::session_only).
    pub fn is_session_only(&self, column: &dyn AnyVec, row: usize) -> bool {
        self.session_only.is_some_and(|holds| holds(column, row))
    }

    /// Returns whether a game save keeps this component although a scene does not.
    ///
    /// Declared with `#[component(resumable)]` on state play resumes from.
    pub fn is_resumed(&self) -> bool {
        self.resumable
    }
}

/// A component read from a save, not yet in any world.
///
/// A load reads every component before it adds any, so one that cannot be read
/// leaves the world as it was.
pub struct StagedComponent {
    /// The component, ready to add.
    pub component: Box<dyn PendingComponent>,
    /// What reading it adapted — paths only; the load names the entity.
    pub report: Vec<ReportEntry>,
}

/// A component value waiting to be added to an entity.
pub trait PendingComponent: Send {
    /// Appends the value as a new row of its page column, so no entity migrates.
    fn push_into(self: Box<Self>, column: &mut dyn AnyVec);

    /// Adds the component to `entity` on its own, outside a page being built.
    ///
    /// # Errors
    ///
    /// When the world cannot add the component to `entity`, with the world's reason.
    fn commit(self: Box<Self>, world: &mut World, entity: EntityId) -> Result<(), String>;

    /// Returns whether `holds` says the value lives only in a session (see
    /// [`ComponentRegistration::session_only`]).
    ///
    /// A value with no Rust type (a component a script declares) never does.
    fn is_session_only(&self, holds: SessionOnlyFn) -> bool {
        let _ = holds;
        false
    }
}

/// Returns the component `reg` describes on `entity`, written as a record.
///
/// `None` when the entity does not have it.
pub fn component_to_record(
    reg: &ComponentRegistration,
    world: &World,
    entity: EntityId,
    references: &mut dyn ReferenceWriter,
) -> Option<Result<Record, RecordError>> {
    let (column, row) = world.component_cell(entity, reg.type_id)?;
    Some((reg.column_to_record)(column, row, references))
}

/// Copies `reg`'s component from `entity` in `src` onto `target` in `dst` through a record.
///
/// Entity references keep their ids, so a component holding one compares equal
/// on both sides.
#[cfg(test)]
pub(crate) fn copy_through_record(
    reg: &ComponentRegistration,
    src: &World,
    entity: EntityId,
    dst: &mut World,
    target: EntityId,
) -> Option<Result<(), String>> {
    use khora_core::ecs::PersistentId;

    use super::record::EntityRef;

    struct Same;
    impl ReferenceWriter for Same {
        fn write_entity(&mut self, entity: EntityId) -> EntityRef {
            EntityRef::Id(PersistentId::from_bits(
                (u64::from(entity.index) << 32) | u64::from(entity.generation),
            ))
        }
    }
    impl ReferenceReader for Same {
        fn read_entity(&mut self, reference: EntityRef) -> Result<EntityId, RecordError> {
            match reference {
                EntityRef::Id(id) => Ok(EntityId {
                    index: (id.to_bits() >> 32) as u32,
                    generation: id.to_bits() as u32,
                }),
                EntityRef::Outside => Err(RecordError("an outside reference".to_owned())),
            }
        }
    }

    let record = match component_to_record(reg, src, entity, &mut Same)? {
        Ok(record) => record,
        Err(error) => return Some(Err(error.to_string())),
    };
    Some(
        (reg.stage)(&record, &mut Same)
            .map_err(|error| error.to_string())
            .and_then(|staged| staged.component.commit(dst, target)),
    )
}

/// The registrations, indexed once: the inventory is fixed at link time.
struct Index {
    by_name: HashMap<&'static str, &'static ComponentRegistration>,
    by_former_name: HashMap<&'static str, &'static ComponentRegistration>,
    by_type: HashMap<TypeId, &'static ComponentRegistration>,
}

fn index() -> &'static Index {
    static INDEX: OnceLock<Index> = OnceLock::new();
    INDEX.get_or_init(|| {
        let mut index = Index {
            by_name: HashMap::new(),
            by_former_name: HashMap::new(),
            by_type: HashMap::new(),
        };
        for reg in inventory::iter::<ComponentRegistration> {
            index.by_name.entry(reg.type_name).or_insert(reg);
            index.by_type.entry(reg.type_id).or_insert(reg);
            for former in reg.formerly {
                index.by_former_name.entry(former).or_insert(reg);
            }
        }
        index
    })
}

/// Returns the registration a save names `type_name` by: its current or a `formerly` name.
pub fn registration_named(type_name: &str) -> Option<&'static ComponentRegistration> {
    let index = index();
    index
        .by_name
        .get(type_name)
        .or_else(|| index.by_former_name.get(type_name))
        .copied()
}

/// Returns the registration of a component by its current `type_name`.
pub fn registration_of(type_name: &str) -> Option<&'static ComponentRegistration> {
    index().by_name.get(type_name).copied()
}

/// Returns the registration of the Rust component `type_id`, if it has one.
pub fn registration_for(type_id: TypeId) -> Option<&'static ComponentRegistration> {
    index().by_type.get(&type_id).copied()
}

/// Returns the registered provenance of a component by its `type_name`.
///
/// `None` for a type that never registered: a `HandleComponent<T>`, or one
/// tagged `#[component(no_serializable)]`.
pub fn provenance_of(type_name: &str) -> Option<ComponentProvenance> {
    registration_of(type_name).map(|reg| reg.provenance)
}

#[cfg(test)]
mod tests {
    use khora_core::math::Vec3;

    use super::*;
    use crate::ecs::component::Component;
    use crate::ecs::{
        AudioSource, Camera, Children, GlobalTransform, Light, Name, Parent, SemanticDomain, Tag,
        Transform,
    };

    /// A test-only anchor: a round-trip target exists without any component the
    /// round trip adds, so `from_json` and `stage` take the real "add" path.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct Anchor;
    impl Component for Anchor {}

    /// Spawns the round-trip target: a live entity carrying only [`Anchor`], in its own domain.
    fn spawn_anchor_target(world: &mut World) -> EntityId {
        world.register_component::<Anchor>(SemanticDomain::Ui);
        world.spawn(Anchor)
    }

    /// Spawns one entity carrying every commonly authored component, with non-default values.
    ///
    /// The fixture of [`json_roundtrip_covers_every_present_component`] and
    /// [`record_roundtrip_covers_every_present_component`].
    fn spawn_representative_entity(world: &mut World) -> EntityId {
        // A second entity so `Parent`/`Children` carry real ids.
        let other = world.spawn((Transform::identity(), GlobalTransform::identity()));

        let mut tags = Tag::new();
        tags.insert("enemy");
        tags.insert("boss");

        world.spawn((
            Transform::from_translation(Vec3::new(1.5, -2.0, 3.25)),
            GlobalTransform::at_position(Vec3::new(1.5, -2.0, 3.25)),
            Name::new("Fixture"),
            Camera::new_perspective(std::f32::consts::FRAC_PI_3, 0.05, 500.0),
            Light::point(),
            AudioSource::default(),
            crate::ecs::AudioListener,
            crate::ecs::ActiveEvents,
            tags,
            Parent(other),
            Children(vec![other]),
        ))
    }

    /// **The inspector's JSON round-trips every component the fixture carries.**
    ///
    /// `to_json` then `from_json` on a fresh entity in a fresh world re-creates
    /// each with the same JSON.
    #[test]
    fn json_roundtrip_covers_every_present_component() {
        let mut src = World::new();
        let entity = spawn_representative_entity(&mut src);

        let mut covered = 0usize;
        for reg in inventory::iter::<ComponentRegistration> {
            let Some(json) = (reg.to_json)(&src, entity) else {
                // The fixture entity doesn't carry this component — nothing to
                // assert for it here.
                continue;
            };
            covered += 1;

            // Fresh world + fresh entity: `from_json` must reconstruct the
            // component from the captured JSON alone.
            let mut dst = World::new();
            let target = spawn_anchor_target(&mut dst);
            (reg.from_json)(&mut dst, target, &json).unwrap_or_else(|e| {
                panic!("from_json failed for {}: {}", reg.type_name, e);
            });

            let restored = (reg.to_json)(&dst, target).unwrap_or_else(|| {
                panic!(
                    "{} missing after from_json — component was not added",
                    reg.type_name
                )
            });
            assert_eq!(
                restored, json,
                "{} JSON did not round-trip through from_json",
                reg.type_name
            );
        }

        // Sanity: the fixture must reach a meaningful slice of the registry,
        // otherwise a silent registration regression could make this test
        // pass vacuously.
        assert!(
            covered >= 6,
            "expected the fixture to cover at least 6 components, got {covered}"
        );
    }

    /// **A record round-trips every component the fixture carries.**
    ///
    /// `column_to_record` then `stage` on a fresh entity leaves the same JSON view.
    #[test]
    fn record_roundtrip_covers_every_present_component() {
        let mut src = World::new();
        let entity = spawn_representative_entity(&mut src);

        for reg in inventory::iter::<ComponentRegistration> {
            let before = (reg.to_json)(&src, entity);
            let mut dst = World::new();
            let target = spawn_anchor_target(&mut dst);
            let Some(copied) = copy_through_record(reg, &src, entity, &mut dst, target) else {
                continue;
            };
            copied.unwrap_or_else(|e| {
                panic!("the record round trip failed for {}: {}", reg.type_name, e);
            });

            let after = (reg.to_json)(&dst, target);
            assert!(
                after.is_some(),
                "{} missing after the record round trip",
                reg.type_name
            );
            assert_eq!(
                after, before,
                "{} did not round-trip through a record",
                reg.type_name
            );
        }
    }

    mod provenance_tests {
        use super::super::*;

        /// Locks the classification of the components whose provenance is not the default.
        ///
        /// One reverted to `Authored` would appear in "Add Component" and be copied
        /// on duplicate.
        #[test]
        fn engine_written_components_are_classified() {
            // Recomputed by `transform_propagation` from Transform + Parent.
            assert_eq!(
                provenance_of("GlobalTransform"),
                Some(ComponentProvenance::Derived)
            );
            // Inverse index of `Parent`, maintained by `World::set_parent`.
            // Copying it would make a duplicate claim the original's children.
            assert_eq!(
                provenance_of("Children"),
                Some(ComponentProvenance::Derived)
            );
            // Set by dragging in the scene tree; persists and must survive a
            // duplicate, but adding an empty one by hand is meaningless.
            assert_eq!(
                provenance_of("Parent"),
                Some(ComponentProvenance::ToolAuthored)
            );
            // Debug output of the physics writeback.
            assert_eq!(
                provenance_of("PhysicsDebugData"),
                Some(ComponentProvenance::Runtime)
            );
        }

        /// No Rust component is resumable: a behavior resumes from its state companion.
        ///
        /// Marking one decides how its domain reads it back on load.
        #[test]
        fn no_rust_component_is_resumed() {
            let resumed: Vec<&str> = inventory::iter::<ComponentRegistration>
                .into_iter()
                .filter(|reg| reg.is_resumed())
                .map(|reg| reg.type_name)
                .collect();
            assert!(resumed.is_empty(), "{resumed:?}");
        }

        /// Resumable is about what the engine wrote: no component an author or a
        /// tool writes is also resumed, so a save never holds a value twice.
        #[test]
        fn a_resumed_component_is_never_also_saved() {
            for reg in inventory::iter::<ComponentRegistration> {
                assert!(
                    !(reg.is_saved() && reg.is_resumed()),
                    "{} is both saved and resumed",
                    reg.type_name
                );
            }
        }

        /// **A marker is a component, and a scene has to record it.**
        ///
        /// `#[component(no_serializable)]` drops the registration, so a scene
        /// saved and loaded back would lose `ActiveEvents` and `AudioListener`.
        #[test]
        fn a_marker_component_is_registered_and_authored() {
            for marker in ["ActiveEvents", "AudioListener"] {
                assert_eq!(
                    provenance_of(marker),
                    Some(ComponentProvenance::Authored),
                    "{marker} must be recorded in a scene — somebody put it there on purpose"
                );
            }
        }

        /// The components a user actually authors keep the default, including the
        /// two whose registration is hand-written rather than derive-generated.
        #[test]
        fn authored_components_keep_the_default() {
            for name in [
                "Transform",
                "Camera",
                "Light",
                "Tag",
                "AssetRef<Mesh>",
                "AssetRef<Box<dyn Material>>",
            ] {
                assert_eq!(
                    provenance_of(name),
                    Some(ComponentProvenance::Authored),
                    "{name} should be author-written"
                );
            }
        }

        /// The two halves of the hierarchy edge are classified differently, and
        /// the asymmetry is the point.
        ///
        /// `Parent` is written by the reparent action, so it persists and a
        /// duplicate keeps it — but nobody adds one from a menu, hence
        /// `ToolAuthored`. `Children` is merely the inverse index rebuilt from it,
        /// so it is `Derived` and must never be copied verbatim.
        #[test]
        fn parent_and_children_are_classified_asymmetrically() {
            let parent = provenance_of("Parent").expect("Parent is registered");
            let children = provenance_of("Children").expect("Children is registered");

            assert!(!parent.is_hand_authorable());
            assert!(parent.is_copied_on_duplicate());

            assert!(!children.is_hand_authorable());
            assert!(!children.is_copied_on_duplicate());
        }
    }
}

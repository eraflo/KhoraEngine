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

//! Asset references as components: saved under their own name in one generic
//! form, through every encoding, a snapshot and the inspector — a value made in
//! code left out of what a file holds.

use khora_core::asset::{
    kinds, AssetKind, AssetUUID, EmissiveMaterial, InlineMaterial, LocalSource, Material,
    ProceduralMesh, ProceduralMeshKind, StandardMaterial, UnlitMaterial, WireframeMaterial,
};
use khora_core::math::LinearRgba;
use khora_core::renderer::api::gpu_scene::Mesh;
use khora_core::script::{ErgonType, ScriptValue};
use khora_data::ecs::{
    AssetRef, ComponentProvenance, FieldKind, Name, PackedField, PackedLayout,
    RuntimeComponentDecl, SemanticDomain, Transform,
};
use khora_data::scene::record::{ReportEntry, ReportKind, VariantPayload};
use khora_data::scene::{capture_world_reporting, ComponentRegistration};

use super::declared_components::{assert_same_rows, attach, with_fields};
use super::snapshot::{load_snapshot, snapshot_of};
use super::*;

/// The name a mesh reference is saved and registered under.
const MESH_REF: &str = "AssetRef<Mesh>";

/// The name a material reference is saved and registered under.
const MATERIAL_REF: &str = "AssetRef<Box<dyn Material>>";

/// A declared component holding a prefab and a sound, as a script declares it.
const GUN: &str = "Gun";

/// The registration named `name`.
fn registration(name: &str) -> &'static ComponentRegistration {
    inventory::iter::<ComponentRegistration>
        .into_iter()
        .find(|reg| reg.type_name == name)
        .unwrap_or_else(|| panic!("no component is registered as `{name}`"))
}

/// One procedural mesh of each primitive.
fn procedural_meshes() -> Vec<ProceduralMesh> {
    vec![
        ProceduralMesh::new(ProceduralMeshKind::Cube, [2.0, 0.0, 0.0, 0.0]),
        ProceduralMesh::new(ProceduralMeshKind::Sphere, [0.5, 24.0, 12.0, 0.0]),
        ProceduralMesh::new(ProceduralMeshKind::Plane, [10.0, -1.0, 0.0, 0.0]),
    ]
}

/// One material of each built-in type, none of them at its defaults.
fn inline_materials() -> Vec<Box<dyn Material>> {
    vec![
        Box::new(StandardMaterial {
            base_color: LinearRgba::new(0.8, 0.1, 0.1, 1.0),
            base_color_texture: Some(AssetUUID::new_v5("textures/wall.png")),
            normal_map: Some(AssetUUID::new_v5("textures/wall_normal.png")),
            metallic: 0.25,
            roughness: 0.6,
            ..StandardMaterial::default()
        }),
        Box::new(UnlitMaterial {
            base_color: LinearRgba::new(0.2, 0.9, 0.3, 1.0),
            ..UnlitMaterial::default()
        }),
        Box::new(EmissiveMaterial {
            emissive_color: LinearRgba::new(1.0, 0.5, 0.0, 1.0),
            intensity: 4.0,
            ..EmissiveMaterial::default()
        }),
        Box::new(WireframeMaterial {
            color: LinearRgba::new(0.0, 0.0, 1.0, 1.0),
            line_width: 2.5,
        }),
    ]
}

/// Every form of a mesh reference.
fn every_mesh_form() -> Vec<AssetRef<Mesh>> {
    let mut forms = vec![
        AssetRef::<Mesh>::Asset(AssetUUID::new_v5("meshes/rock.gltf")),
        AssetRef::<Mesh>::Made(AssetUUID::new()),
    ];
    forms.extend(procedural_meshes().into_iter().map(AssetRef::<Mesh>::Local));
    forms
}

/// Every form of a material reference.
fn every_material_form() -> Vec<AssetRef<Box<dyn Material>>> {
    let mut forms = vec![
        AssetRef::<Box<dyn Material>>::Asset(AssetUUID::new_v5("materials/rock.kmat")),
        AssetRef::<Box<dyn Material>>::Made(AssetUUID::new()),
    ];
    forms.extend(
        inline_materials()
            .into_iter()
            .map(|material| AssetRef::<Box<dyn Material>>::Local(InlineMaterial::new(material))),
    );
    forms
}

/// A material reference's form and uuid: a material has no equality, its
/// content uuid stands for it.
fn material_identity(material: &AssetRef<Box<dyn Material>>) -> (&'static str, AssetUUID) {
    let form = match material {
        AssetRef::Asset(_) => "Asset",
        AssetRef::Local(_) => "Local",
        AssetRef::Made(_) => "Made",
    };
    (form, material.uuid())
}

fn optional_asset(kind: AssetKind) -> ErgonType {
    ErgonType::Optional(Box::new(ErgonType::Asset(kind)))
}

/// Declares `Gun { Prefab? bullet; Sound? shot; }` in `world`.
fn declare_gun(world: &mut World) {
    let fields = [
        ("bullet", optional_asset(kinds::PREFAB)),
        ("shot", optional_asset(kinds::SOUND)),
    ];
    let layout = PackedLayout::typed(
        fields
            .iter()
            .map(|(name, ty)| PackedField {
                name: (*name).to_owned(),
                kind: FieldKind::of(ty),
                default: ScriptValue::Null,
            })
            .collect(),
        fields.iter().map(|(_, ty)| ty.clone()).collect(),
    )
    .expect("a valid layout");
    world
        .register_runtime_component(RuntimeComponentDecl {
            name: GUN.to_owned(),
            domain: SemanticDomain::Script,
            provenance: ComponentProvenance::Authored,
            resumable: false,
            layout,
        })
        .expect("`Gun` registers");
}

/// A world holding `meshes` and `materials`, one authored entity each, and
/// the entities in that order.
fn world_of(
    meshes: &[AssetRef<Mesh>],
    materials: &[AssetRef<Box<dyn Material>>],
) -> (World, Vec<EntityId>) {
    let mut world = World::new();
    let mut entities = Vec::new();
    for mesh in meshes {
        entities.push(world.spawn((Transform::identity(), mesh.clone())));
    }
    for material in materials {
        entities.push(world.spawn((Transform::identity(), material.clone())));
    }
    for &entity in &entities {
        world.mark_authored(entity).expect("authored");
    }
    (world, entities)
}

/// The entity of `dst` known by `entity`'s identity in `src`.
fn twin_of(src: &World, dst: &World, entity: EntityId) -> EntityId {
    let id = src.persistent_id(entity).expect("an id");
    dst.entity_with_id(id)
        .unwrap_or_else(|| panic!("no entity known as {id:?} after the load"))
}

/// Asserts each entity of `src` holding a mesh or a material reference holds
/// the same one in `dst`, under the same identity.
fn assert_same_references(src: &World, dst: &World, entities: &[EntityId], case: &str) {
    for &entity in entities {
        let twin = twin_of(src, dst, entity);
        if let Some(mesh) = src.get::<AssetRef<Mesh>>(entity) {
            assert_eq!(
                dst.get::<AssetRef<Mesh>>(twin),
                Some(mesh),
                "{case}: a mesh reference"
            );
        }
        if let Some(material) = src.get::<AssetRef<Box<dyn Material>>>(entity) {
            assert_eq!(
                dst.get::<AssetRef<Box<dyn Material>>>(twin)
                    .map(material_identity),
                Some(material_identity(material)),
                "{case}: a material reference"
            );
        }
    }
}

/// Every copy of a material's JSON `value` with exactly one leaf changed.
///
/// A number moves, a flag flips, an empty texture slot names a texture; a
/// string (a texture's uuid or an alpha mode) becomes each other value it can
/// hold, those that do not read back being skipped by the caller.
fn with_one_field_changed(value: &serde_json::Value) -> Vec<serde_json::Value> {
    use serde_json::{json, Value};

    let texture = serde_json::to_value(AssetUUID::new_v5("textures/other.png")).expect("a uuid");
    match value {
        Value::Number(number) => vec![json!(number.as_f64().expect("a number") + 0.5)],
        Value::Bool(flag) => vec![Value::Bool(!flag)],
        Value::Null => vec![texture],
        Value::String(_) => vec![
            texture,
            json!("Opaque"),
            json!("Blend"),
            json!({ "Mask": 0.5 }),
        ],
        Value::Array(items) => (0..items.len())
            .flat_map(|at| {
                with_one_field_changed(&items[at])
                    .into_iter()
                    .map(move |changed| {
                        let mut items = items.clone();
                        items[at] = changed;
                        Value::Array(items)
                    })
            })
            .collect(),
        Value::Object(fields) => fields
            .iter()
            .flat_map(|(name, field)| {
                with_one_field_changed(field)
                    .into_iter()
                    .map(move |changed| {
                        let mut fields = fields.clone();
                        fields.insert(name.clone(), changed);
                        Value::Object(fields)
                    })
            })
            .collect(),
    }
}

/// Every form a file holds — a project asset, procedural meshes of each
/// primitive, inline materials of each type, a declared component's prefab
/// and sound — saves and loads back equal in every encoding, nothing adapted.
#[test]
fn every_asset_ref_form_round_trips_through_each_encoding() {
    let meshes: Vec<_> = every_mesh_form()
        .into_iter()
        .filter(|form| !form.is_made())
        .collect();
    let materials: Vec<_> = every_material_form()
        .into_iter()
        .filter(|form| !form.is_made())
        .collect();
    let (mut src, mut entities) = world_of(&meshes, &materials);
    declare_gun(&mut src);
    let gun = src.spawn(Transform::identity());
    attach(
        &mut src,
        gun,
        GUN,
        with_fields(&[
            (
                "bullet",
                ScriptValue::Asset(AssetUUID::new_v5("prefabs/bullet.kprefab")),
            ),
            (
                "shot",
                ScriptValue::Asset(AssetUUID::new_v5("sounds/shot.ogg")),
            ),
        ]),
    );
    src.mark_authored(gun).expect("authored");
    entities.push(gun);

    for (name, encoding) in every_encoding() {
        let (dst, applied) = reload(&src, name, encoding);
        assert!(
            applied.report.is_clean(),
            "{name}: a same-version round trip adapted something: {:?}",
            applied.report.entries
        );
        assert_same_world(&src, &dst, name);
        assert_same_references(&src, &dst, &entities, name);
        assert_same_rows(&src, &dst, &[GUN], name);
    }
}

/// A mesh or a material reference is one component under its own name: its
/// registration, its vtable and the name a world finds it by, and the column
/// a record holds it in. No other name stands for it.
#[test]
fn an_asset_ref_component_is_saved_under_its_own_name() {
    let world = World::new();
    for (name, type_id) in [
        (MESH_REF, std::any::TypeId::of::<AssetRef<Mesh>>()),
        (
            MATERIAL_REF,
            std::any::TypeId::of::<AssetRef<Box<dyn Material>>>(),
        ),
    ] {
        let reg = registration(name);
        assert_eq!(reg.type_id, type_id, "`{name}`: the registered type");

        let key = world
            .components()
            .key_named(name)
            .unwrap_or_else(|| panic!("a world knows no `{name}`"));
        let vtable = world.components().vtable(key).expect("a vtable");
        assert_eq!(&*vtable.name, name, "the vtable's name");
        assert_eq!(
            vtable.rust.map(|rust| rust.type_id),
            Some(type_id),
            "`{name}`: the vtable's type"
        );
    }

    let (src, entities) = world_of(
        &[AssetRef::<Mesh>::Asset(AssetUUID::new())],
        &[AssetRef::<Box<dyn Material>>::Asset(AssetUUID::new())],
    );
    let record = capture_world(&src).expect("captures");
    let mesh = src.persistent_id(entities[0]).expect("an id");
    let material = src.persistent_id(entities[1]).expect("an id");
    assert!(
        slot_of(&record, mesh, MESH_REF).is_some(),
        "the mesh's column"
    );
    assert!(
        slot_of(&record, material, MATERIAL_REF).is_some(),
        "the material's column"
    );

    for retired in ["MeshRef", "MaterialRef"] {
        assert!(
            !saved_registrations().any(|reg| reg.type_name == retired),
            "`{retired}` still names a component"
        );
    }
}

/// A local source is named by its content: equal procedural parameters or
/// equal inline materials give one uuid, different ones another — and a
/// reference carrying the source names that uuid.
#[test]
fn a_local_source_names_itself_by_its_content() {
    let uuid = |mesh: &ProceduralMesh| LocalSource::<Mesh>::uuid(mesh);
    let sphere = ProceduralMesh::new(ProceduralMeshKind::Sphere, [0.5, 24.0, 12.0, 0.0]);
    let same_sphere = ProceduralMesh::new(ProceduralMeshKind::Sphere, [0.5, 24.0, 12.0, 0.0]);
    let finer_sphere = ProceduralMesh::new(ProceduralMeshKind::Sphere, [0.5, 32.0, 16.0, 0.0]);
    let cube = ProceduralMesh::new(ProceduralMeshKind::Cube, [0.5, 24.0, 12.0, 0.0]);
    assert_eq!(uuid(&sphere), uuid(&same_sphere), "equal parameters");
    assert_ne!(uuid(&sphere), uuid(&finer_sphere), "other parameters");
    assert_ne!(uuid(&sphere), uuid(&cube), "another primitive");
    assert_eq!(
        AssetRef::<Mesh>::Local(sphere.clone()).uuid(),
        uuid(&sphere)
    );

    let uuid = |material: &InlineMaterial| LocalSource::<Box<dyn Material>>::uuid(material);
    let red = || {
        InlineMaterial::new(Box::new(StandardMaterial {
            base_color: LinearRgba::new(1.0, 0.0, 0.0, 1.0),
            ..StandardMaterial::default()
        }))
    };
    let blue = InlineMaterial::new(Box::new(StandardMaterial {
        base_color: LinearRgba::new(0.0, 0.0, 1.0, 1.0),
        ..StandardMaterial::default()
    }));
    let standard = InlineMaterial::new(Box::new(StandardMaterial::default()));
    let unlit = InlineMaterial::new(Box::new(UnlitMaterial::default()));
    assert_eq!(uuid(&red()), uuid(&red()), "equal content");
    assert_ne!(uuid(&red()), uuid(&blue), "other content");
    assert_ne!(uuid(&standard), uuid(&unlit), "another material type");
    assert_eq!(
        AssetRef::<Box<dyn Material>>::Local(red()).uuid(),
        uuid(&red())
    );
}

/// Every field of an inline material names it: a material of each built-in
/// type differing from another in any one field — a colour channel, a factor,
/// a flag, a texture slot, the alpha mode — has another uuid, so two such
/// materials never share one copy.
#[test]
fn every_field_of_an_inline_material_feeds_its_uuid() {
    use khora_data::scene::{material_from_json, material_to_json};

    for material in inline_materials() {
        let json = material_to_json(&*material).expect("a built-in material has a JSON form");
        let uuid = InlineMaterial::new(material.clone_box()).uuid();
        let mut changed_fields = 0;
        for changed in with_one_field_changed(&json["material"]) {
            let mut other_json = json.clone();
            other_json["material"] = changed;
            let Ok(other) = material_from_json(&other_json) else {
                continue;
            };
            if material_to_json(&*other).as_ref() == Some(&json) {
                continue;
            }
            changed_fields += 1;
            assert_ne!(
                InlineMaterial::new(other).uuid(),
                uuid,
                "a material differing in one field shares its uuid: {other_json}"
            );
        }
        assert!(
            changed_fields > 0,
            "no field of {} was changed",
            json["type_name"]
        );
    }
}

/// A mesh or a material made in code lives only in the session: a scene
/// leaves it out and reports it `NotSaved` on its entity, the entity saved
/// with its other components; a snapshot, read in the same session, keeps it.
#[test]
fn a_made_reference_of_every_kind_is_session_only() {
    let mesh = AssetUUID::new();
    let material = AssetUUID::new();
    let mut world = World::new();
    let with_mesh = world.spawn((
        Transform::identity(),
        Name::new("Made mesh"),
        AssetRef::<Mesh>::Made(mesh),
    ));
    let with_material = world.spawn((
        Transform::identity(),
        Name::new("Made material"),
        AssetRef::<Box<dyn Material>>::Made(material),
    ));
    let mesh_id = world.mark_authored(with_mesh).expect("authored");
    let material_id = world.mark_authored(with_material).expect("authored");

    let (record, left_out) = capture_world_reporting(&world).expect("captures");

    let not_saved = |entity: PersistentId, component: &str| ReportEntry {
        entity: Some(entity),
        component: Some(component.to_owned()),
        path: String::new(),
        kind: ReportKind::NotSaved,
    };
    let mut left_out = left_out;
    left_out.sort_by_key(|entry| entry.component.clone());
    let mut expected = vec![
        not_saved(mesh_id, MESH_REF),
        not_saved(material_id, MATERIAL_REF),
    ];
    expected.sort_by_key(|entry| entry.component.clone());
    assert_eq!(left_out, expected, "what the scene left out");
    assert_eq!(slot_of(&record, mesh_id, MESH_REF), None, "the made mesh");
    assert_eq!(
        slot_of(&record, material_id, MATERIAL_REF),
        None,
        "the made material"
    );
    for id in [mesh_id, material_id] {
        assert!(
            slot_of(&record, id, "Name").is_some(),
            "{id:?} lost its name"
        );
    }

    let file = snapshot_of(&world);
    let mut restored = World::new();
    let applied = load_snapshot(&mut restored, &file)
        .unwrap_or_else(|e| panic!("the snapshot did not load: {e}"));
    assert!(
        applied.report.is_clean(),
        "a snapshot adapted a value made in code: {:?}",
        applied.report.entries
    );
    let mesh_twin = restored.entity_with_id(mesh_id).expect("restored");
    let material_twin = restored.entity_with_id(material_id).expect("restored");
    assert_eq!(
        restored.get::<AssetRef<Mesh>>(mesh_twin),
        Some(&AssetRef::<Mesh>::Made(mesh))
    );
    assert_eq!(
        restored
            .get::<AssetRef<Box<dyn Material>>>(material_twin)
            .map(material_identity),
        Some(("Made", material)),
    );
}

/// A material made in code written into a scene record (by hand: no save
/// writes one) names nothing a load can find: the load drops it from its
/// entity and reports it `Dropped`; the entity keeps its other components,
/// and its page neighbour its own material.
#[test]
fn a_made_material_in_a_file_is_dropped() {
    let authored = AssetUUID::new_v5("materials/rock.kmat");
    let mut src = World::new();
    let kept = src.spawn((
        Transform::identity(),
        Name::new("Kept"),
        AssetRef::<Box<dyn Material>>::Asset(authored),
    ));
    let forged = src.spawn((
        Transform::identity(),
        Name::new("Forged"),
        AssetRef::<Box<dyn Material>>::Asset(AssetUUID::new()),
    ));
    let kept_id = src.mark_authored(kept).expect("authored");
    let forged_id = src.mark_authored(forged).expect("authored");
    let mut record = capture_world(&src).expect("captures");
    match value_mut(&mut record, forged_id, MATERIAL_REF) {
        Record::Variant { variant, .. } => *variant = "Made".to_owned(),
        other => panic!("a material reference is recorded as a variant, got {other:?}"),
    }

    let mut dst = World::new();
    let applied = apply(&mut dst, &record, Identity::Keep).expect("the record loads");

    let forged = dst
        .entity_with_id(forged_id)
        .expect("the forged entity loads");
    let kept = dst.entity_with_id(kept_id).expect("the kept entity loads");
    assert!(
        dst.get::<AssetRef<Box<dyn Material>>>(forged).is_none(),
        "the load kept a material made in code read from a file"
    );
    assert_eq!(
        dst.get::<Name>(forged).map(Name::as_str),
        Some("Forged"),
        "the entity lost its other components"
    );
    assert_eq!(
        dst.get::<AssetRef<Box<dyn Material>>>(kept)
            .map(material_identity),
        Some(("Asset", authored)),
        "its page neighbour lost its material"
    );
    assert!(
        applied.report.entries.contains(&ReportEntry {
            entity: Some(forged_id),
            component: Some(MATERIAL_REF.to_owned()),
            path: String::new(),
            kind: ReportKind::Dropped,
        }),
        "the load does not report the material it refused: {:?}",
        applied.report.entries
    );
}

/// Every form — a made value included — goes through a snapshot and back
/// unchanged, and both references have a complete snapshot schema, so a
/// snapshot is never refused for them.
#[test]
fn asset_refs_round_trip_through_a_snapshot() {
    for name in [MESH_REF, MATERIAL_REF] {
        assert!(
            (registration(name).schema_complete)(),
            "`{name}` has an incomplete snapshot schema"
        );
    }

    let (src, entities) = world_of(&every_mesh_form(), &every_material_form());
    let file = snapshot_of(&src);
    let mut dst = World::new();
    let applied =
        load_snapshot(&mut dst, &file).unwrap_or_else(|e| panic!("the snapshot did not load: {e}"));

    assert!(
        applied.report.is_clean(),
        "a snapshot adapted something: {:?}",
        applied.report.entries
    );
    assert_same_references(&src, &dst, &entities, "snapshot");
}

/// The inspector's JSON of every form reads back into the same reference; an
/// asset and a made value are written `{"Asset": uuid}` and `{"Made": uuid}`.
#[test]
fn inspector_json_round_trips_each_form() {
    let (src, entities) = world_of(&every_mesh_form(), &every_material_form());
    let mut dst = World::new();
    for &entity in &entities {
        let name = if src.get::<AssetRef<Mesh>>(entity).is_some() {
            MESH_REF
        } else {
            MATERIAL_REF
        };
        let reg = registration(name);
        let json = (reg.to_json)(&src, entity)
            .unwrap_or_else(|| panic!("`{name}` of {entity:?} has no JSON form"));
        let target = dst.spawn(Transform::identity());
        (reg.from_json)(&mut dst, target, &json)
            .unwrap_or_else(|e| panic!("`{name}` does not read its own JSON {json}: {e}"));

        assert_eq!(
            (reg.to_json)(&dst, target).as_ref(),
            Some(&json),
            "`{name}`: the JSON read back writes another"
        );
        if let Some(mesh) = src.get::<AssetRef<Mesh>>(entity) {
            assert_eq!(dst.get::<AssetRef<Mesh>>(target), Some(mesh), "{json}");
        }
        if let Some(material) = src.get::<AssetRef<Box<dyn Material>>>(entity) {
            assert_eq!(
                dst.get::<AssetRef<Box<dyn Material>>>(target)
                    .map(material_identity),
                Some(material_identity(material)),
                "{json}"
            );
        }
    }

    let uuid = AssetUUID::new_v5("meshes/rock.gltf");
    let uuid_json = serde_json::to_value(uuid).expect("a uuid is JSON");
    let (forms, entities) = world_of(
        &[AssetRef::<Mesh>::Asset(uuid), AssetRef::<Mesh>::Made(uuid)],
        &[
            AssetRef::<Box<dyn Material>>::Asset(uuid),
            AssetRef::<Box<dyn Material>>::Made(uuid),
        ],
    );
    let expected = [
        (MESH_REF, serde_json::json!({ "Asset": uuid_json })),
        (MESH_REF, serde_json::json!({ "Made": uuid_json })),
        (MATERIAL_REF, serde_json::json!({ "Asset": uuid_json })),
        (MATERIAL_REF, serde_json::json!({ "Made": uuid_json })),
    ];
    for (&entity, (name, json)) in entities.iter().zip(expected) {
        assert_eq!((registration(name).to_json)(&forms, entity), Some(json));
    }
}

/// What a load adapted inside an inline material is reported at its place in
/// the reference: `Inline.material.<field>`.
#[test]
fn load_reports_keep_their_paths() {
    let mut src = World::new();
    let entity = src.spawn((
        Transform::identity(),
        AssetRef::<Box<dyn Material>>::Local(InlineMaterial::new(Box::new(StandardMaterial {
            metallic: 0.25,
            ..StandardMaterial::default()
        }))),
    ));
    let id = src.mark_authored(entity).expect("authored");
    let mut record = capture_world(&src).expect("captures");

    // An older material, without `specular`, and a newer one's `sparkle`.
    match value_mut(&mut record, id, MATERIAL_REF) {
        Record::Variant {
            variant,
            payload: VariantPayload::Struct(fields),
            ..
        } if variant == "Inline" => {
            let material = fields
                .iter_mut()
                .find_map(|(name, value)| (name == "material").then_some(value))
                .expect("an inline reference holds its material");
            match material {
                Record::Struct { fields, .. } => {
                    let before = fields.len();
                    fields.retain(|(name, _)| name != "specular");
                    assert_eq!(fields.len(), before - 1, "the material saves `specular`");
                    fields.push(("sparkle".to_owned(), Record::F32(0.5)));
                }
                other => panic!("an inline material is recorded field by field, got {other:?}"),
            }
        }
        other => panic!("an inline material reference is recorded as its variant, got {other:?}"),
    }

    let mut dst = World::new();
    let applied = apply(&mut dst, &record, Identity::Keep).expect("the record loads");

    let at = |path: &str, kind: ReportKind| ReportEntry {
        entity: Some(id),
        component: Some(MATERIAL_REF.to_owned()),
        path: path.to_owned(),
        kind,
    };
    for entry in [
        at("Inline.material.specular", ReportKind::Defaulted),
        at("Inline.material.sparkle", ReportKind::Dropped),
    ] {
        assert!(
            applied.report.entries.contains(&entry),
            "{entry:?} is not reported: {:?}",
            applied.report.entries
        );
    }
    let twin = dst.entity_with_id(id).expect("the entity loads");
    assert!(
        matches!(
            dst.get::<AssetRef<Box<dyn Material>>>(twin),
            Some(AssetRef::Local(_))
        ),
        "the damaged material still loads"
    );
}

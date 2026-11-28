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

//! The resolver reads the references that changed since its last run and
//! the decodes that landed since; it puts its copies in the asset store only.

use std::fs;
use std::path::Path;

use khora_core::asset::{InlineMaterial, ProceduralMesh, ProceduralMeshKind};
use khora_core::math::Vec3;
use khora_data::ecs::{Observer, Transform};
use khora_telemetry::MetricsRegistry;
use tempfile::TempDir;

use super::*;
use crate::asset::{FileLoader, IndexBuilder};

const RESOLVER: &str = "asset_resolver";

/// A project assets dir on disk, served by a real `AssetService` in a runtime.
///
/// The service has the registered decoders; the runtime holds a shared
/// `AssetStore` and the reserve the `loading` family stages into.
struct Project {
    dir: TempDir,
    service: Arc<Mutex<AssetService>>,
    store: AssetStore,
    runtime: Runtime,
}

impl Project {
    fn new(files: &[(&str, Vec<u8>)]) -> Self {
        let dir = tempfile::tempdir().expect("a temporary project");
        for (rel, bytes) in files {
            write_file(dir.path(), rel, bytes);
        }
        let index_bytes = IndexBuilder::new(crate::trust::FileGuard::new(
            dir.path(),
            crate::trust::Trust::Project,
            crate::trust::Refusals::detached(),
        ))
        .build_index_bytes()
        .expect("the project indexes");
        let mut service = AssetService::new(
            &index_bytes,
            Box::new(FileLoader::new(dir.path())),
            Arc::new(MetricsRegistry::new()),
            None,
        )
        .expect("an asset service over the project");
        service.register_inventory_decoders();
        let service = Arc::new(Mutex::new(service));
        let store = AssetStore::new();
        let mut runtime = Runtime::default();
        runtime.resources.insert(store.clone());
        runtime.resources.insert(khora_core::lane::Reserve::new());
        runtime.services.insert(service.clone());
        Self {
            dir,
            service,
            store,
            runtime,
        }
    }

    /// Runs the registered data system `name` once, as the scheduler's runner does.
    ///
    /// It is handed the tick of its last run; its writes carry this one.
    fn run_system(&self, world: &mut World, name: &'static str) {
        let observer = Observer::DataSystem(name);
        let ctx = khora_data::ecs::SystemContext {
            since: world.observer_last_seen(observer),
            resumed: world.observer_resumes(observer),
            scope: None,
        };
        let tick = world.begin_run();
        let system = inventory::iter::<DataSystemRegistration>
            .into_iter()
            .find(|system| system.name == name)
            .unwrap_or_else(|| panic!("`{name}` is registered"));
        (system.run)(world, &self.runtime, &mut OutputDeck::default(), ctx);
        world.note_observer_run(observer, tick);
        world.end_run();
    }

    /// One frame of the asset systems, whose resolver run reports the reference rows it read.
    ///
    /// The boundary's intake, then the registered reload and the
    /// resolver. Nothing is decoded during it: what it files waits for
    /// [`Self::decode`].
    fn counted_frame(&self, world: &mut World) -> usize {
        crate::systems::asset_intake::take_in_decoded(&self.runtime);
        self.run_system(world, "asset_reload");
        let observer = Observer::DataSystem(RESOLVER);
        let since = world.observer_last_seen(observer);
        let tick = world.begin_run();
        let visited = resolve_references(world, &self.runtime, since);
        world.note_observer_run(observer, tick);
        world.end_run();
        visited
    }

    /// Frames until nothing more is filed, each followed by the `loading` family's run.
    fn settle(&self, world: &mut World) {
        crate::systems::asset_intake::tests::settle(&self.runtime, world, |world| {
            self.run_system(world, "asset_reload");
            self.run_system(world, RESOLVER);
        });
    }

    /// One run of the `loading` family between two frames; returns how
    /// many decodes it took.
    fn decode(&self) -> usize {
        crate::systems::asset_intake::tests::decode_filed(&self.runtime)
    }

    fn write(&self, rel: &str, bytes: &[u8]) {
        write_file(self.dir.path(), rel, bytes);
    }

    /// Announces the asset `uuid` changed, as the editor's hot reload does.
    fn invalidate(&self, uuid: &AssetUUID) {
        self.service
            .lock()
            .expect("the service's lock")
            .invalidate(uuid);
    }

    fn stored<A: Stored>(&self, uuid: &AssetUUID) -> Option<AssetHandle<A>> {
        self.store
            .store::<A>()
            .read()
            .expect("the store's lock")
            .get(uuid)
            .cloned()
    }

    /// The roughness of the `StandardMaterial` the store holds under `uuid`.
    fn stored_roughness(&self, uuid: &AssetUUID) -> Option<f32> {
        self.stored::<Box<dyn Material>>(uuid).map(|material| {
            let material: &dyn Material = &**material;
            material
                .as_any()
                .downcast_ref::<StandardMaterial>()
                .expect("a StandardMaterial")
                .roughness
        })
    }
}

fn write_file(root: &Path, rel: &str, bytes: &[u8]) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().expect("a parent folder")).expect("the folders");
    fs::write(path, bytes).expect("the file is written");
}

/// A real PNG of `width` x `height`, encoded in memory.
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    let img = image::RgbaImage::from_pixel(width, height, image::Rgba([90, 60, 30, 255]));
    image::DynamicImage::ImageRgba8(img)
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .expect("encode png");
    bytes
}

/// The `.kmat` RON form of a `StandardMaterial` of `roughness`, whose
/// base-colour slot names `base_color_texture` when it is `Some`.
fn kmat(roughness: f32, base_color_texture: Option<AssetUUID>) -> Vec<u8> {
    let texture = match base_color_texture {
        Some(uuid) => {
            let tex = serde_json::to_value(uuid)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .expect("AssetUUID serializes to a string");
            format!("\"{tex}\"")
        }
        None => "()".to_owned(),
    };
    format!(
        r#"{{
                    "type_name": "StandardMaterial",
                    "material": {{
                        "base_color": {{ "r": 1.0, "g": 1.0, "b": 1.0, "a": 1.0 }},
                        "base_color_texture": {texture},
                        "metallic": 0.0,
                        "roughness": {roughness:?},
                        "metallic_roughness_texture": (),
                        "normal_map": (),
                        "occlusion_map": (),
                        "emissive": {{ "r": 0.0, "g": 0.0, "b": 0.0, "a": 1.0 }},
                        "emissive_texture": (),
                        "alpha_mode": "Opaque",
                        "alpha_cutoff": 0.5,
                        "double_sided": false,
                    }},
                }}"#
    )
    .into_bytes()
}

fn cube(size: f32) -> AssetRef<Mesh> {
    AssetRef::<Mesh>::Local(ProceduralMesh::new(
        ProceduralMeshKind::Cube,
        [size, 0.0, 0.0, 0.0],
    ))
}

fn tinted(red: f32) -> AssetRef<Box<dyn Material>> {
    AssetRef::<Box<dyn Material>>::Local(InlineMaterial::new(Box::new(StandardMaterial {
        base_color: LinearRgba::new(red, 0.5, 0.5, 1.0),
        ..Default::default()
    })))
}

/// The resolver watches both references, so a write to either is a change it reads.
#[test]
fn the_resolver_observes_mesh_and_material_references() {
    let resolver = inventory::iter::<DataSystemRegistration>
        .into_iter()
        .find(|system| system.name == RESOLVER)
        .expect("the resolver is registered");
    let observed: Vec<ComponentKey> = resolver.observes.iter().map(|key| key()).collect();
    for (key, name) in [
        (ComponentKey::of::<AssetRef<Mesh>>(), "AssetRef<Mesh>"),
        (
            ComponentKey::of::<AssetRef<Box<dyn Material>>>(),
            "AssetRef<Box<dyn Material>>",
        ),
    ] {
        assert!(observed.contains(&key), "observes {name}: {observed:?}");
    }
}

/// A `AssetRef<Mesh>` and a `AssetRef<Box<dyn Material>>` given other values are resolved on the next run.
///
/// Their new copies are in the store, and only their two rows are read.
#[test]
fn a_changed_reference_is_resolved_on_the_next_run() {
    let project = Project::new(&[]);
    let mut world = World::new();
    let entity = world.spawn((cube(1.0), tinted(0.1)));
    project.counted_frame(&mut world);
    project.counted_frame(&mut world);
    watch::<Mesh>(&project.store);
    watch::<Box<dyn Material>>(&project.store);

    let (mesh, material) = (cube(4.0), tinted(0.9));
    let (mesh_uuid, material_uuid) = (mesh.uuid(), material.uuid());
    assert!(
        world.set_component(entity, mesh),
        "the AssetRef<Mesh> is replaced"
    );
    assert!(
        world.set_component(entity, material),
        "the AssetRef<Box<dyn Material>> is replaced"
    );
    let visited = project.counted_frame(&mut world);

    assert_eq!(visited, 2, "the two changed references are read");
    assert!(
        project.stored::<Mesh>(&mesh_uuid).is_some(),
        "the new mesh copy is in the store"
    );
    assert!(
        project
            .stored::<Box<dyn Material>>(&material_uuid)
            .is_some(),
        "the new material copy is in the store"
    );
    assert_eq!(
        arrived::<Mesh>(&project.store),
        vec![Arrival {
            uuid: mesh_uuid,
            kind: ArrivalKind::Landed,
        }],
        "the new mesh lands under its uuid"
    );
    assert_eq!(
        arrived::<Box<dyn Material>>(&project.store),
        vec![Arrival {
            uuid: material_uuid,
            kind: ArrivalKind::Landed,
        }],
        "the new material lands under its uuid"
    );
}

/// Once every reference is resolved, the resolver reads no reference row.
///
/// Not on a frame where nothing changed, nor on one where only another
/// component of the entities did.
#[test]
fn an_unchanged_reference_is_not_visited() {
    let kmat_path = "materials/plain.kmat";
    let project = Project::new(&[(kmat_path, kmat(0.5, None))]);
    let mut world = World::new();
    let asset_material = AssetRef::<Box<dyn Material>>::Asset(AssetUUID::new_v5(kmat_path));
    let entities: Vec<EntityId> = (0..6)
        .map(|i| {
            let material = if i % 2 == 0 {
                tinted(i as f32 / 10.0)
            } else {
                asset_material.clone()
            };
            world.spawn((Transform::default(), cube(1.0 + i as f32), material))
        })
        .collect();
    project.settle(&mut world);
    project.counted_frame(&mut world);
    watch::<Mesh>(&project.store);
    watch::<Box<dyn Material>>(&project.store);

    for frame in 0..3 {
        assert_eq!(
            project.counted_frame(&mut world),
            0,
            "frame {frame}: no reference changed, none is read"
        );
    }
    for (i, entity) in entities.iter().enumerate() {
        let mut transform = world
            .get_mut::<Transform>(*entity)
            .expect("the entity has a Transform");
        transform.translation = Vec3::new(i as f32, 0.0, 0.0);
    }
    assert_eq!(
        project.counted_frame(&mut world),
        0,
        "a moved entity's references are not read"
    );
    assert!(
        arrived::<Mesh>(&project.store).is_empty(),
        "no mesh copy is stored again"
    );
    assert!(
        arrived::<Box<dyn Material>>(&project.store).is_empty(),
        "no material copy is stored again"
    );
}

/// A material still decoding gets its copy into the store the frame its decode lands.
///
/// The texture it names is asked for then, all without the reference
/// changing. The landed copy reaches the store through the boundary's
/// intake; the resolver still owes the material its textures.
#[test]
fn a_pending_decode_is_resolved_when_it_lands() {
    let tex_path = "textures/late.png";
    let kmat_path = "materials/late.kmat";
    let tex = AssetUUID::new_v5(tex_path);
    let mat = AssetUUID::new_v5(kmat_path);
    let project = Project::new(&[(tex_path, png(3, 2)), (kmat_path, kmat(0.4, Some(tex)))]);
    let mut world = World::new();
    world.spawn((cube(1.0), AssetRef::<Box<dyn Material>>::Asset(mat)));

    assert_eq!(project.counted_frame(&mut world), 2, "the new references");
    assert!(
        project.stored::<Box<dyn Material>>(&mat).is_none(),
        "the premise: the material is still decoding"
    );
    assert_eq!(project.decode(), 1, "the material is filed for decoding");

    let visited = project.counted_frame(&mut world);
    assert_eq!(
        project.stored_roughness(&mat),
        Some(0.4),
        "the material is in the store the frame its decode lands"
    );
    assert_eq!(
        visited, 0,
        "the reference did not change: its row is not read"
    );
    assert_eq!(
        project.decode(),
        1,
        "the texture the landed material names is asked for"
    );

    project.counted_frame(&mut world);
    let texture = project
        .stored::<CpuTexture>(&tex)
        .expect("the material's texture is in the store once it lands");
    assert_eq!((texture.size.width, texture.size.height), (3, 2));
}

/// A material whose decode failed is not decoded again on the next
/// frames; once its file changes it is, and its copy reaches the store.
#[test]
fn a_failed_decode_waits_for_its_file_to_change() {
    let kmat_path = "materials/broken.kmat";
    let mat = AssetUUID::new_v5(kmat_path);
    let project = Project::new(&[(kmat_path, b"definitely not a material".to_vec())]);
    let mut world = World::new();
    world.spawn(AssetRef::<Box<dyn Material>>::Asset(mat));

    project.counted_frame(&mut world);
    assert_eq!(project.decode(), 1, "the material is filed for decoding");
    for frame in 0..4 {
        project.counted_frame(&mut world);
        assert_eq!(
            project.decode(),
            0,
            "frame {frame}: the failed material is not filed again"
        );
        assert!(project.stored::<Box<dyn Material>>(&mat).is_none());
    }

    project.write(kmat_path, &kmat(0.7, None));
    project.invalidate(&mat);
    project.counted_frame(&mut world);
    assert_eq!(
        project.decode(),
        1,
        "the changed file is decoded again, the reference unchanged"
    );
    project.counted_frame(&mut world);
    assert_eq!(
        project.stored_roughness(&mat),
        Some(0.7),
        "the fixed material is in the store"
    );
}

/// No entity is given a CPU copy, and no component type exists to hold
/// one: the store holds every copy, also after a hot reload.
#[test]
fn no_entity_carries_a_cpu_handle() {
    let kmat_path = "materials/plain.kmat";
    let mat = AssetUUID::new_v5(kmat_path);
    let project = Project::new(&[(kmat_path, kmat(0.5, None))]);
    let mut world = World::new();
    for name in [
        "HandleComponent<Mesh>",
        "HandleComponent<Box<dyn Material>>",
    ] {
        assert_eq!(
            world.components().key_named(name),
            None,
            "`{name}` is not a registered component"
        );
    }
    let (inline, procedural) = (tinted(0.3), cube(2.0));
    let (inline_uuid, procedural_uuid) = (inline.uuid(), procedural.uuid());
    let entities = [
        world.spawn((procedural, AssetRef::<Box<dyn Material>>::Asset(mat))),
        world.spawn((cube(3.0), inline)),
    ];
    project.settle(&mut world);

    assert!(project.stored::<Mesh>(&procedural_uuid).is_some());
    assert!(project.stored::<Box<dyn Material>>(&inline_uuid).is_some());
    assert_eq!(project.stored_roughness(&mat), Some(0.5));
    project.write(kmat_path, &kmat(0.8, None));
    project.invalidate(&mat);
    project.settle(&mut world);
    assert_eq!(
        project.stored_roughness(&mat),
        Some(0.8),
        "the reloaded material replaces its copy in the store"
    );
    for entity in entities {
        assert!(
            world.get::<HandleComponent<Mesh>>(entity).is_none(),
            "{entity:?} carries no CPU mesh"
        );
        assert!(
            world
                .get::<HandleComponent<Box<dyn Material>>>(entity)
                .is_none(),
            "{entity:?} carries no CPU material"
        );
    }
}

/// A `AssetRef<Box<dyn Material>>` added to an entity the resolver already read is a
/// change: it is resolved on the next run, its row the only one read.
#[test]
fn a_reference_added_to_an_existing_entity_is_resolved() {
    let project = Project::new(&[]);
    let mut world = World::new();
    let entity = world.spawn(cube(1.0));
    project.counted_frame(&mut world);
    assert_eq!(project.counted_frame(&mut world), 0, "the premise: settled");

    let material = tinted(0.6);
    let uuid = material.uuid();
    world
        .add_component(entity, material)
        .expect("the AssetRef<Box<dyn Material>> is added");
    let visited = project.counted_frame(&mut world);

    assert_eq!(visited, 1, "the added reference is read");
    assert!(
        project.stored::<Box<dyn Material>>(&uuid).is_some(),
        "the added material's copy is in the store"
    );
}

/// A material made in code is never asked of a file: missing from the
/// store, it waits for nothing; once code stores it, its textures load.
#[test]
fn a_material_made_in_code_is_asked_of_no_file() {
    let tex_path = "textures/made.png";
    let tex = AssetUUID::new_v5(tex_path);
    let project = Project::new(&[(tex_path, png(2, 2))]);
    let made = AssetUUID::new();
    let mut world = World::new();
    world.spawn((cube(1.0), AssetRef::<Box<dyn Material>>::Made(made)));
    project.settle(&mut world);

    assert!(
        project.stored::<Box<dyn Material>>(&made).is_none(),
        "nothing stands in for a made material the store lacks"
    );
    let waited = project
        .service
        .lock()
        .expect("the service's lock")
        .take_resolving();
    assert!(
        waited.iter().all(|(uuid, _)| *uuid != made),
        "a made material is waited for as a file: {waited:?}"
    );

    project
        .store
        .store::<Box<dyn Material>>()
        .write()
        .expect("the store's lock")
        .insert(
            made,
            AssetHandle::new(Box::new(StandardMaterial {
                base_color_texture: Some(tex),
                ..Default::default()
            }) as Box<dyn Material>),
        );
    project.settle(&mut world);
    assert!(
        project.stored::<CpuTexture>(&tex).is_some(),
        "the texture of a made material is loaded once it is stored"
    );
}

/// A reference moved from one form to another names each form's copy in turn:
/// a project asset, a local source by its content, a value made in code.
#[test]
fn a_reference_moved_between_forms_names_each_copy() {
    let kmat_path = "materials/plain.kmat";
    let mat = AssetUUID::new_v5(kmat_path);
    let project = Project::new(&[(kmat_path, kmat(0.5, None))]);
    let mut world = World::new();
    let entity = world.spawn((cube(1.0), AssetRef::<Box<dyn Material>>::Asset(mat)));
    project.settle(&mut world);
    assert_eq!(project.stored_roughness(&mat), Some(0.5), "the asset");

    let local = tinted(0.2);
    let local_uuid = local.uuid();
    assert!(world.set_component(entity, local));
    assert_eq!(
        project.counted_frame(&mut world),
        1,
        "the local form is read"
    );
    assert!(
        project.stored::<Box<dyn Material>>(&local_uuid).is_some(),
        "the local source's copy, under its content uuid"
    );

    let made = AssetUUID::new();
    project
        .store
        .store::<Box<dyn Material>>()
        .write()
        .expect("the store's lock")
        .insert(
            made,
            AssetHandle::new(Box::new(StandardMaterial::default()) as Box<dyn Material>),
        );
    assert!(world.set_component(entity, AssetRef::<Box<dyn Material>>::Made(made)));
    assert_eq!(
        project.counted_frame(&mut world),
        1,
        "the made form is read"
    );

    assert!(world.set_component(entity, AssetRef::<Box<dyn Material>>::Asset(mat)));
    project.settle(&mut world);
    assert_eq!(project.stored_roughness(&mat), Some(0.5), "the asset again");
    assert!(
        project.stored::<Box<dyn Material>>(&made).is_some(),
        "the made copy stays the store's"
    );
}

/// What the resolver keeps for an asset once no reference names it.
mod references_removed {
    use super::*;

    /// The assets the resolver still waits for, taken from the service.
    fn waited_for(project: &Project) -> HashSet<(AssetUUID, TypeId)> {
        project
            .service
            .lock()
            .expect("the service's lock")
            .take_resolving()
    }

    /// An asset that failed, or that no file holds, is not waited for
    /// once the last reference naming it is gone.
    #[test]
    #[ignore = "a known defect: the wanted set keeps assets no reference names; the generic resolver's liveness pruning (phase G2 of the plan) fixes it"]
    fn an_asset_no_reference_names_is_not_waited_for() {
        let broken_path = "materials/broken.kmat";
        let broken = AssetUUID::new_v5(broken_path);
        let missing = AssetUUID::new_v5("meshes/never_there.obj");
        let project = Project::new(&[(broken_path, b"not a material".to_vec())]);
        let mut world = World::new();
        let entity = world.spawn((
            AssetRef::<Mesh>::Asset(missing),
            AssetRef::<Box<dyn Material>>::Asset(broken),
        ));
        project.counted_frame(&mut world);
        project.decode();
        project.counted_frame(&mut world);

        world.despawn(entity);
        project.counted_frame(&mut world);
        project.counted_frame(&mut world);

        assert_eq!(
            waited_for(&project),
            HashSet::new(),
            "nothing names the failed material nor the missing mesh"
        );
    }

    /// A failed asset fixed on disk after its last reference left is not decoded.
    #[test]
    #[ignore = "a known defect: a failed asset no reference names is decoded again on any change; the event-driven wanted set (phase G2) fixes it"]
    fn a_fixed_asset_no_reference_names_is_not_decoded() {
        let kmat_path = "materials/broken.kmat";
        let mat = AssetUUID::new_v5(kmat_path);
        let project = Project::new(&[(kmat_path, b"not a material".to_vec())]);
        let mut world = World::new();
        let entity = world.spawn(AssetRef::<Box<dyn Material>>::Asset(mat));
        project.counted_frame(&mut world);
        assert_eq!(project.decode(), 1, "the premise: filed once");
        project.counted_frame(&mut world);
        world.despawn(entity);
        project.counted_frame(&mut world);

        project.write(kmat_path, &kmat(0.7, None));
        project.invalidate(&mat);
        project.counted_frame(&mut world);

        assert_eq!(project.decode(), 0, "no reference names the material");
        project.counted_frame(&mut world);
        assert!(
            project.stored::<Box<dyn Material>>(&mat).is_none(),
            "no copy is made of what nothing names"
        );
    }

    /// An inline material edited again and again leaves only the copy its
    /// reference names in the store.
    #[test]
    #[ignore = "a known defect: no CPU copy is ever freed; the liveness rule over CPU copies (phase G5) fixes it"]
    fn an_edited_inline_material_leaves_no_copy_behind() {
        let project = Project::new(&[]);
        let mut world = World::new();
        let entity = world.spawn(tinted(0.0));
        project.counted_frame(&mut world);

        for edit in 1..=50 {
            assert!(world.set_component(entity, tinted(edit as f32 / 100.0)));
            project.counted_frame(&mut world);
        }

        let held = project
            .store
            .store::<Box<dyn Material>>()
            .read()
            .expect("the store's lock")
            .len();
        assert_eq!(held, 1, "only the material the reference names is held");
    }
}

/// References resolved when the store's arrivals overflowed the resolver's reading.
mod arrivals_lost {
    use khora_core::asset::{ProceduralMesh, ProceduralMeshKind};

    use super::*;

    /// Lists the project's folder again into its service, as a file
    /// appearing or leaving does.
    fn reindex(project: &Project) {
        let bytes = IndexBuilder::new(crate::trust::FileGuard::new(
            project.dir.path(),
            crate::trust::Trust::Project,
            crate::trust::Refusals::detached(),
        ))
        .build_index_bytes()
        .expect("the project indexes");
        project
            .service
            .lock()
            .expect("the service's lock")
            .reindex(&bytes)
            .expect("the index is replaced");
    }

    /// A mesh whose file left in a frame of more arrivals than the
    /// journal keeps is asked for again once its file is back.
    #[test]
    #[ignore = "a known defect: a Gone lost in a resync is never wanted again; the resolver's resync recovery (phase G2) fixes it"]
    fn a_mesh_gone_in_a_flood_of_arrivals_comes_back_with_its_file() {
        let obj_path = "meshes/back.obj";
        let mesh = AssetUUID::new_v5(obj_path);
        let triangle = b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n".to_vec();
        let project = Project::new(&[(obj_path, triangle.clone())]);
        project
            .service
            .lock()
            .expect("the service's lock")
            .register_decoder::<Mesh>(
                khora_core::asset::kinds::MESH.name(),
                crate::asset::MeshDispatcher::default(),
            );
        let mut world = World::new();
        world.spawn(AssetRef::<Mesh>::Asset(mesh));
        project.settle(&mut world);
        assert!(project.stored::<Mesh>(&mesh).is_some(), "the premise");

        // A large scene's decodes land at the boundary the file leaves on.
        {
            let other = AssetHandle::new(
                ProceduralMesh::new(ProceduralMeshKind::Cube, [1.0, 0.0, 0.0, 0.0]).build(),
            );
            let meshes = project.store.store::<Mesh>();
            let mut meshes = meshes.write().expect("the store's lock");
            for _ in 0..khora_data::assets::ARRIVALS_KEPT {
                meshes.insert(AssetUUID::new(), other.clone());
            }
        }
        fs::remove_file(project.dir.path().join(obj_path)).expect("removed");
        reindex(&project);
        project.invalidate(&mesh);
        project.counted_frame(&mut world);
        assert!(project.stored::<Mesh>(&mesh).is_none(), "the premise: gone");

        project.write(obj_path, &triangle);
        reindex(&project);
        project.invalidate(&mesh);
        project.settle(&mut world);

        assert!(
            project.stored::<Mesh>(&mesh).is_some(),
            "the mesh the entity names is back in the store"
        );
    }
}

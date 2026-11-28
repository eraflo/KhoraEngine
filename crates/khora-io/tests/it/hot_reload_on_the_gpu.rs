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

//! A changed mesh or texture file reaches the GPU copies every entity naming
//! it draws, through the copy the reload replaces in the asset store: no
//! entity is given or stripped of anything.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use khora_core::asset::{
    AssetDecoding, AssetHandle, AssetUUID, InlineMaterial, Material, ProceduralMesh,
    StandardMaterial,
};
use khora_core::ecs::entity::EntityId;
use khora_core::lane::{OutputDeck, Reserve, Staging};
use khora_core::math::dimension::{Extent3D, Origin3D};
use khora_core::renderer::api::command::{
    BindGroupDescriptor, BindGroupId, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
    BindGroupLayoutId, CommandBufferId,
};
use khora_core::renderer::api::device::GraphicsAdapterInfo;
use khora_core::renderer::api::gpu_scene::{GpuMesh, Mesh};
use khora_core::renderer::api::material::GpuMaterial;
use khora_core::renderer::api::pipeline::{
    ComputePipelineDescriptor, ComputePipelineId, ComputePipelineSpec, LayoutKey,
    PipelineLayoutDescriptor, PipelineLayoutId, PipelineSpec, RenderPipelineDescriptor,
    RenderPipelineId,
};
use khora_core::renderer::api::resource::{
    BufferDescriptor, BufferId, SamplerDescriptor, SamplerId, TextureDescriptor, TextureFormat,
    TextureId, TextureViewDescriptor, TextureViewId,
};
use khora_core::renderer::api::shader::{ShaderModuleDescriptor, ShaderModuleId, ShaderVariantKey};
use khora_core::renderer::error::{RenderError, ResourceError};
use khora_core::renderer::traits::{CommandEncoder, GraphicsDevice, PipelineSystem};
use khora_core::Runtime;
use khora_data::ecs::{
    AssetRef, DataSystemRegistration, HandleComponent, Observer, SystemContext, World,
};
use khora_data::{AssetEviction, AssetStore, ProjectionRegistry};
use khora_io::asset::{AssetIdRegistry, AssetService};
use khora_telemetry::MetricsRegistry;

/// A triangle, as an OBJ file.
const TRIANGLE_OBJ: &str = "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";
/// A quad: the decoder gives each of its two triangles its own vertices.
const QUAD_OBJ: &str = "v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nf 1 2 3 4\n";

/// What the device was asked to make and give back.
#[derive(Debug, Default)]
struct Ledger {
    buffers_with_data: Vec<(BufferId, Vec<u8>)>,
    destroyed_buffers: Vec<BufferId>,
    textures: Vec<(TextureId, Extent3D)>,
    views: Vec<(TextureViewId, TextureId)>,
}

/// A device that makes whatever it is asked to and records the buffers made
/// with data, the buffers given back, and each texture with its views.
#[derive(Debug, Default)]
struct GpuLedger {
    next_id: AtomicUsize,
    ledger: Mutex<Ledger>,
}

impl GpuLedger {
    fn id(&self) -> usize {
        self.next_id.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn ledger(&self) -> std::sync::MutexGuard<'_, Ledger> {
        self.ledger.lock().expect("the device ledger")
    }

    /// The bytes `buffer` was made with, if it was made with data.
    fn data_of(&self, buffer: BufferId) -> Option<Vec<u8>> {
        self.ledger()
            .buffers_with_data
            .iter()
            .find(|(id, _)| *id == buffer)
            .map(|(_, bytes)| bytes.clone())
    }

    /// Returns how many buffers were made holding exactly `bytes`.
    fn uploads_of(&self, bytes: &[u8]) -> usize {
        self.ledger()
            .buffers_with_data
            .iter()
            .filter(|(_, data)| data.as_slice() == bytes)
            .count()
    }

    fn destroyed_buffers(&self) -> Vec<BufferId> {
        self.ledger().destroyed_buffers.clone()
    }

    /// The size of the texture `view` was made on.
    fn size_behind(&self, view: TextureViewId) -> Option<Extent3D> {
        let ledger = self.ledger();
        let texture = ledger
            .views
            .iter()
            .find(|(v, _)| *v == view)
            .map(|(_, texture)| *texture)?;
        ledger
            .textures
            .iter()
            .find(|(t, _)| *t == texture)
            .map(|(_, size)| *size)
    }
}

impl GraphicsDevice for GpuLedger {
    fn create_shader_module(
        &self,
        _d: &ShaderModuleDescriptor,
    ) -> Result<ShaderModuleId, ResourceError> {
        Ok(ShaderModuleId(self.id()))
    }
    fn destroy_shader_module(&self, _id: ShaderModuleId) -> Result<(), ResourceError> {
        Ok(())
    }
    fn create_render_pipeline(
        &self,
        _d: &RenderPipelineDescriptor,
    ) -> Result<RenderPipelineId, ResourceError> {
        Ok(RenderPipelineId(self.id()))
    }
    fn create_pipeline_layout(
        &self,
        _d: &PipelineLayoutDescriptor,
    ) -> Result<PipelineLayoutId, ResourceError> {
        Ok(PipelineLayoutId(self.id()))
    }
    fn destroy_render_pipeline(&self, _id: RenderPipelineId) -> Result<(), ResourceError> {
        Ok(())
    }
    fn create_compute_pipeline(
        &self,
        _d: &ComputePipelineDescriptor,
    ) -> Result<ComputePipelineId, ResourceError> {
        Ok(ComputePipelineId(self.id() as u64))
    }
    fn destroy_compute_pipeline(&self, _id: ComputePipelineId) -> Result<(), ResourceError> {
        Ok(())
    }
    fn create_bind_group_layout(
        &self,
        _d: &BindGroupLayoutDescriptor,
    ) -> Result<BindGroupLayoutId, ResourceError> {
        Ok(BindGroupLayoutId(self.id()))
    }
    fn create_bind_group(&self, _d: &BindGroupDescriptor) -> Result<BindGroupId, ResourceError> {
        Ok(BindGroupId(self.id()))
    }
    fn destroy_bind_group_layout(&self, _id: BindGroupLayoutId) -> Result<(), ResourceError> {
        Ok(())
    }
    fn destroy_bind_group(&self, _id: BindGroupId) -> Result<(), ResourceError> {
        Ok(())
    }
    fn create_buffer(&self, _d: &BufferDescriptor) -> Result<BufferId, ResourceError> {
        Ok(BufferId(self.id()))
    }
    fn create_buffer_with_data(
        &self,
        _d: &BufferDescriptor,
        data: &[u8],
    ) -> Result<BufferId, ResourceError> {
        let buffer = BufferId(self.id());
        self.ledger()
            .buffers_with_data
            .push((buffer, data.to_vec()));
        Ok(buffer)
    }
    fn destroy_buffer(&self, id: BufferId) -> Result<(), ResourceError> {
        self.ledger().destroyed_buffers.push(id);
        Ok(())
    }
    fn write_buffer(&self, _id: BufferId, _offset: u64, _data: &[u8]) -> Result<(), ResourceError> {
        Ok(())
    }
    fn write_buffer_async<'a>(
        &'a self,
        _id: BufferId,
        _offset: u64,
        _data: &'a [u8],
    ) -> Box<dyn Future<Output = Result<(), ResourceError>> + Send + 'static> {
        Box::new(async { Ok(()) })
    }
    fn create_texture(&self, d: &TextureDescriptor) -> Result<TextureId, ResourceError> {
        let texture = TextureId(self.id());
        self.ledger().textures.push((texture, d.size));
        Ok(texture)
    }
    fn destroy_texture(&self, _id: TextureId) -> Result<(), ResourceError> {
        Ok(())
    }
    fn write_texture(
        &self,
        _texture_id: TextureId,
        _data: &[u8],
        _bytes_per_row: Option<u32>,
        _offset: Origin3D,
        _size: Extent3D,
    ) -> Result<(), ResourceError> {
        Ok(())
    }
    fn create_texture_view(
        &self,
        texture_id: TextureId,
        _d: &TextureViewDescriptor,
    ) -> Result<TextureViewId, ResourceError> {
        let view = TextureViewId(self.id());
        self.ledger().views.push((view, texture_id));
        Ok(view)
    }
    fn destroy_texture_view(&self, _id: TextureViewId) -> Result<(), ResourceError> {
        Ok(())
    }
    fn create_sampler(&self, _d: &SamplerDescriptor) -> Result<SamplerId, ResourceError> {
        Ok(SamplerId(self.id()))
    }
    fn destroy_sampler(&self, _id: SamplerId) -> Result<(), ResourceError> {
        Ok(())
    }
    fn create_command_encoder(&self, _label: Option<&str>) -> Box<dyn CommandEncoder> {
        unimplemented!("a residency records no command")
    }
    fn submit_command_buffer(&self, _command_buffer: CommandBufferId) {}
    fn get_surface_format(&self) -> Option<TextureFormat> {
        Some(TextureFormat::Rgba8UnormSrgb)
    }
    fn get_surface_size(&self) -> (u32, u32) {
        (800, 600)
    }
    fn get_adapter_info(&self) -> GraphicsAdapterInfo {
        GraphicsAdapterInfo::default()
    }
    fn supports_feature(&self, _feature_name: &str) -> bool {
        false
    }
}

/// A pipeline system whose every material layout is one fixed id.
#[derive(Debug, Default)]
struct MaterialLayouts;

impl PipelineSystem for MaterialLayouts {
    fn layout(
        &self,
        _device: &dyn GraphicsDevice,
        _key: LayoutKey,
        _variant: &ShaderVariantKey,
    ) -> Result<BindGroupLayoutId, RenderError> {
        Ok(BindGroupLayoutId(0))
    }
    fn inline_layout(
        &self,
        _device: &dyn GraphicsDevice,
        _label: &'static str,
        _entries: &[BindGroupLayoutEntry],
    ) -> Result<BindGroupLayoutId, RenderError> {
        unreachable!("a residency builds no inline layout")
    }
    fn pipeline(
        &self,
        _device: &dyn GraphicsDevice,
        _spec: &PipelineSpec,
    ) -> Result<RenderPipelineId, RenderError> {
        unreachable!("a residency builds no pipeline")
    }
    fn compute_pipeline(
        &self,
        _device: &dyn GraphicsDevice,
        _spec: &ComputePipelineSpec,
    ) -> Result<ComputePipelineId, RenderError> {
        unreachable!("a residency builds no pipeline")
    }
}

/// The registered data system called `name`.
fn data_system(name: &str) -> &'static DataSystemRegistration {
    inventory::iter::<DataSystemRegistration>
        .into_iter()
        .find(|system| system.name == name)
        .unwrap_or_else(|| panic!("`{name}` is not registered"))
}

/// A real square PNG of `side` pixels, of one colour.
fn png(side: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        side,
        side,
        image::Rgba([200, 100, 50, 255]),
    ))
    .write_to(
        &mut std::io::Cursor::new(&mut bytes),
        image::ImageFormat::Png,
    )
    .expect("encodes a png");
    bytes
}

/// The `.kmat` form of a default `StandardMaterial` whose base colour is the texture `texture`.
fn kmat(texture: AssetUUID) -> Vec<u8> {
    let texture = serde_json::to_value(texture)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .expect("an AssetUUID serializes to a string");
    format!(
        r#"{{
            "type_name": "StandardMaterial",
            "material": {{
                "base_color": {{ "r": 1.0, "g": 1.0, "b": 1.0, "a": 1.0 }},
                "base_color_texture": "{texture}",
                "metallic": 0.0,
                "roughness": 0.5,
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

fn write_file(root: &Path, rel: &str, bytes: &[u8]) {
    let path = root.join("assets").join(rel);
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("makes the folder");
    std::fs::write(path, bytes).expect("writes the file");
}

/// A project on disk, opened as the editor opens one and served into a
/// runtime with a device, the asset store and its GPU projection, the
/// material layouts, the eviction, and the reserve the `loading` family
/// stages its decodes into.
struct Project {
    _dir: tempfile::TempDir,
    root: PathBuf,
    service: Arc<Mutex<AssetService>>,
    store: AssetStore,
    device: Arc<GpuLedger>,
    reserve: Reserve,
    runtime: Runtime,
}

impl Project {
    fn with_files(files: &[(&str, Vec<u8>)]) -> Self {
        let dir = tempfile::tempdir().expect("a temp dir");
        let root = dir.path().to_path_buf();
        for (rel, bytes) in files {
            write_file(&root, rel, bytes);
        }
        let service = AssetService::open_loose_files(
            &root,
            &AssetIdRegistry::load_or_unread(&khora_io::trust::FileGuard::new(
                &root,
                khora_io::trust::Trust::Project,
                khora_io::trust::Refusals::detached(),
            )),
            Arc::new(MetricsRegistry::new()),
        )
        .expect("the project opens");
        let service = Arc::new(Mutex::new(service));
        let store = AssetStore::new();
        let device = Arc::new(GpuLedger::default());
        let reserve = Reserve::new();
        let mut runtime = Runtime::default();
        runtime.resources.insert(store.clone());
        runtime
            .resources
            .insert(ProjectionRegistry::new(store.clone()));
        runtime
            .resources
            .insert(Arc::new(MaterialLayouts) as Arc<dyn PipelineSystem>);
        runtime
            .resources
            .insert(Arc::new(Mutex::new(AssetEviction::new())));
        runtime.resources.insert(reserve.clone());
        runtime.services.insert(Arc::clone(&service));
        runtime
            .backends
            .insert(Arc::clone(&device) as Arc<dyn GraphicsDevice>);
        Self {
            _dir: dir,
            root,
            service,
            store,
            device,
            reserve,
            runtime,
        }
    }

    /// One frame of the systems an asset goes through: the boundary's intake,
    /// the `PreExtract` reload, resolver and residencies, and the eviction that
    /// ends the frame; each handed the tick of its last run, as the runner does.
    fn frame(&self, world: &mut World) {
        let mut deck = OutputDeck::new();
        for name in [
            "asset_intake",
            "asset_reload",
            "asset_resolver",
            "gpu_mesh_sync",
            "gpu_material_sync",
            "asset_eviction",
        ] {
            let observer = Observer::DataSystem(name);
            let ctx = SystemContext {
                since: world.observer_last_seen(observer),
                resumed: world.observer_resumes(observer),
                scope: None,
            };
            let tick = world.begin_run();
            (data_system(name).run)(world, &self.runtime, &mut deck, ctx);
            world.note_observer_run(observer, tick);
            world.end_run();
        }
    }

    /// One run of the `loading` family, played by hand: the decodes the frames
    /// filed, decoded under their lease and received by the reserve.
    ///
    /// Returns how many it took.
    fn decode_filed(&self) -> usize {
        let filed = self
            .service
            .lock()
            .expect("the service")
            .decode_requests()
            .drain();
        let decoding: Arc<dyn AssetDecoding> = AssetService::decoding(&self.service);
        let mut staging = Staging::new();
        for request in &filed {
            let (decoded, lease) = decoding.decode_leased(request);
            match lease {
                Some(lease) => staging.stage_while(vec![decoded], lease),
                None => staging.stage(vec![decoded]),
            }
        }
        self.reserve.receive(staging);
        filed.len()
    }

    /// Frames, with the `loading` family's run between them, until a frame
    /// files no decode, then one more frame.
    fn settle(&self, world: &mut World) {
        for _ in 0..8 {
            self.frame(world);
            if self.decode_filed() == 0 {
                self.frame(world);
                return;
            }
        }
        panic!("the frames still file decodes after 8 frames");
    }

    /// What the editor does when the file at `rel` is saved again: the new
    /// bytes on disk, the asset announced as changed.
    fn save(&self, rel: &str, bytes: &[u8]) {
        write_file(&self.root, rel, bytes);
        self.service
            .lock()
            .expect("the service")
            .invalidate(&AssetUUID::new_v5(rel));
    }

    fn stored_mesh(&self, uuid: AssetUUID) -> Option<AssetHandle<Mesh>> {
        self.store
            .store::<Mesh>()
            .read()
            .expect("the mesh store")
            .get(&uuid)
            .cloned()
    }

    fn gpu_mesh(&self, uuid: AssetUUID) -> Option<AssetHandle<GpuMesh>> {
        self.store
            .store::<GpuMesh>()
            .read()
            .expect("the gpu mesh store")
            .get(&uuid)
            .cloned()
    }

    fn gpu_material(&self, uuid: AssetUUID) -> Option<AssetHandle<GpuMaterial>> {
        self.store
            .store::<GpuMaterial>()
            .read()
            .expect("the gpu material store")
            .get(&uuid)
            .cloned()
    }
}

/// Asserts no entity of `entities` carries a GPU mesh or GPU material tag.
fn assert_untagged(world: &World, entities: &[EntityId]) {
    for entity in entities {
        assert!(
            world.get::<HandleComponent<GpuMesh>>(*entity).is_none(),
            "{entity:?} carries no GPU mesh tag"
        );
        assert!(
            world.get::<HandleComponent<GpuMaterial>>(*entity).is_none(),
            "{entity:?} carries no GPU material tag"
        );
    }
}

/// **A hot reload redraws every entity without visiting them.**
///
/// Three entities name one mesh file. Once the file changes and its new
/// copy lands, the GPU copy under that uuid holds the new vertices, made once
/// for the three, and the old copy's buffers are given back. The entities
/// hold their references only: drawing by uuid, all three draw the new copy.
#[test]
fn hot_reload_redraws_every_entity_without_visiting_them() {
    let path = "meshes/shape.obj";
    let uuid = AssetUUID::new_v5(path);
    let project = Project::with_files(&[(path, TRIANGLE_OBJ.as_bytes().to_vec())]);
    let mut world = World::new();
    let entities: Vec<EntityId> = (0..3)
        .map(|_| world.spawn(AssetRef::<Mesh>::Asset(uuid)))
        .collect();
    project.settle(&mut world);
    let Some(first) = project.gpu_mesh(uuid) else {
        panic!("the triangle is resident");
    };

    project.save(path, QUAD_OBJ.as_bytes());
    project.settle(&mut world);

    let stored = project.stored_mesh(uuid).expect("the store holds the mesh");
    assert_eq!(stored.positions.len(), 6, "the store holds the quad");
    let vertices = stored.create_vertex_buffer();
    let Some(second) = project.gpu_mesh(uuid) else {
        panic!("the quad is resident under the same uuid");
    };
    assert_eq!(
        project.device.data_of(second.vertex_buffer),
        Some(vertices.clone()),
        "the GPU copy every entity draws holds the quad"
    );
    assert_eq!(
        project.device.uploads_of(&vertices),
        1,
        "the quad is made resident once for the three entities"
    );
    let destroyed = project.device.destroyed_buffers();
    assert!(
        destroyed.contains(&first.vertex_buffer),
        "the triangle's vertex buffer is given back: {destroyed:?}"
    );
    for entity in &entities {
        assert_eq!(
            world
                .get::<AssetRef<Mesh>>(*entity)
                .map(AssetRef::<Mesh>::uuid),
            Some(uuid),
            "{entity:?} still names the mesh"
        );
    }
    assert_untagged(&world, &entities);
}

/// **References edited away and back are drawn from a GPU copy each time.**
///
/// An inline material and a procedural mesh name their content: an edit
/// makes the new copy resident and the one nothing names is evicted.
/// Edited back, the copy the store still holds is made resident again,
/// though it does not arrive in the store again.
#[test]
fn references_edited_back_are_made_resident_again() {
    use khora_core::asset::ProceduralMeshKind;

    let project = Project::with_files(&[("meshes/shape.obj", TRIANGLE_OBJ.as_bytes().to_vec())]);
    let material = |roughness: f32| {
        AssetRef::<Box<dyn Material>>::Local(InlineMaterial::new(Box::new(StandardMaterial {
            roughness,
            ..Default::default()
        })))
    };
    let cube = |side: f32| {
        AssetRef::<Mesh>::Local(ProceduralMesh::new(
            ProceduralMeshKind::Cube,
            [side, 0.0, 0.0, 0.0],
        ))
    };
    let (first_material, second_material) = (material(0.2).uuid(), material(0.8).uuid());
    let (first_mesh, second_mesh) = (cube(1.0).uuid(), cube(3.0).uuid());
    let mut world = World::new();
    let entity = world.spawn((cube(1.0), material(0.2)));
    project.settle(&mut world);
    assert!(
        project.gpu_mesh(first_mesh).is_some() && project.gpu_material(first_material).is_some(),
        "the premise: the first mesh and material are resident"
    );

    assert!(world.set_component(entity, cube(3.0)));
    assert!(world.set_component(entity, material(0.8)));
    project.settle(&mut world);
    assert!(
        project.gpu_mesh(second_mesh).is_some() && project.gpu_material(second_material).is_some(),
        "the edited mesh and material are resident"
    );
    assert!(
        project.gpu_mesh(first_mesh).is_none() && project.gpu_material(first_material).is_none(),
        "the copies nothing names are evicted"
    );

    assert!(world.set_component(entity, cube(1.0)));
    assert!(world.set_component(entity, material(0.2)));
    project.settle(&mut world);
    assert!(
        project.gpu_mesh(first_mesh).is_some(),
        "the mesh edited back is resident again"
    );
    assert!(
        project.gpu_material(first_material).is_some(),
        "the material edited back is resident again"
    );
}

/// **A repainted texture remakes, on the GPU, every material naming it.**
///
/// One entity names a `.kmat` whose base colour is the image, another an
/// inline material naming the same image. Once the image is saved larger
/// and lands, both GPU materials are made again, bound to the new image.
#[test]
fn a_repainted_texture_remakes_every_material_naming_it_on_the_gpu() {
    let image = "textures/paint.png";
    let texture = AssetUUID::new_v5(image);
    let kmat_path = "materials/painted.kmat";
    let by_asset = AssetUUID::new_v5(kmat_path);
    let mesh_path = "meshes/shape.obj";
    let mesh = AssetUUID::new_v5(mesh_path);
    let project = Project::with_files(&[
        (image, png(2)),
        (kmat_path, kmat(texture)),
        (mesh_path, TRIANGLE_OBJ.as_bytes().to_vec()),
    ]);
    let inline =
        AssetRef::<Box<dyn Material>>::Local(InlineMaterial::new(Box::new(StandardMaterial {
            base_color_texture: Some(texture),
            ..Default::default()
        })));
    let inline_uuid = inline.uuid();
    let mut world = World::new();
    let entities = vec![
        world.spawn((
            AssetRef::<Mesh>::Asset(mesh),
            AssetRef::<Box<dyn Material>>::Asset(by_asset),
        )),
        world.spawn((AssetRef::<Mesh>::Asset(mesh), inline)),
    ];
    project.settle(&mut world);
    let before: Vec<AssetHandle<GpuMaterial>> = [by_asset, inline_uuid]
        .iter()
        .map(|uuid| {
            project
                .gpu_material(*uuid)
                .unwrap_or_else(|| panic!("the material {uuid:?} is resident"))
        })
        .collect();

    project.save(image, &png(5));
    project.settle(&mut world);

    for (uuid, old) in [by_asset, inline_uuid].into_iter().zip(before) {
        let Some(new) = project.gpu_material(uuid) else {
            panic!("the material {uuid:?} is resident again");
        };
        assert_ne!(
            new.bind_group, old.bind_group,
            "the material {uuid:?} is made again"
        );
        let view = new
            .base_color_view
            .expect("the material binds its base colour texture");
        assert_eq!(
            project.device.size_behind(view).map(|size| size.width),
            Some(5),
            "the material {uuid:?} binds the repainted image"
        );
    }
    assert_untagged(&world, &entities);
}

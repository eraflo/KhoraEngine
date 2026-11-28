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

//! A UI image reaches the GPU through the `image_on_gpu` residency, and
//! reaches it again when its file changes: the residency runs after
//! `asset_reload`, which replaced the engine's copy of the changed image.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use khora_core::asset::{AssetDecoding, AssetUUID};
use khora_core::ecs::entity::EntityId;
use khora_core::lane::{OutputDeck, Reserve, Staging};
use khora_core::math::dimension::{Extent3D, Origin3D};
use khora_core::math::Vec2;
use khora_core::renderer::api::command::{
    BindGroupDescriptor, BindGroupId, BindGroupLayoutDescriptor, BindGroupLayoutId, CommandBufferId,
};
use khora_core::renderer::api::device::GraphicsAdapterInfo;
use khora_core::renderer::api::pipeline::{
    ComputePipelineDescriptor, ComputePipelineId, PipelineLayoutDescriptor, PipelineLayoutId,
    RenderPipelineDescriptor, RenderPipelineId,
};
use khora_core::renderer::api::resource::{
    BufferDescriptor, BufferId, CpuTexture, SamplerDescriptor, SamplerId, TextureDescriptor,
    TextureFormat, TextureId, TextureViewDescriptor, TextureViewId,
};
use khora_core::renderer::api::shader::{ShaderModuleDescriptor, ShaderModuleId};
use khora_core::renderer::error::ResourceError;
use khora_core::renderer::traits::{CommandEncoder, GraphicsDevice};
use khora_core::Runtime;
use khora_data::ecs::{DataSystemRegistration, World};
use khora_data::ui::components::{UiImage, UiTransform};
use khora_data::ui::UiImageAtlas;
use khora_data::AssetStore;
use khora_io::asset::{AssetIdRegistry, AssetService};
use khora_telemetry::MetricsRegistry;

/// The image the tests show, relative to the project's assets folder.
const IMAGE: &str = "ui/badge.png";

/// One texture upload: the texture, where in it, its size and its bytes.
#[derive(Debug, Clone)]
struct Upload {
    texture: TextureId,
    offset: Origin3D,
    size: Extent3D,
    bytes: Vec<u8>,
}

/// A device that creates whatever it is asked to and records every texture
/// made, upload and texture or view destroyed.
#[derive(Debug, Default)]
struct UploadLog {
    next_id: AtomicUsize,
    uploads: Mutex<Vec<Upload>>,
    textures: Mutex<Vec<TextureId>>,
    destroyed_textures: Mutex<Vec<TextureId>>,
    destroyed_views: Mutex<Vec<TextureViewId>>,
}

impl UploadLog {
    fn id(&self) -> usize {
        self.next_id.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// The uploads of a `width`×`height` image, in order.
    fn uploads_of(&self, width: u32, height: u32) -> Vec<Upload> {
        self.uploads
            .lock()
            .expect("the upload log")
            .iter()
            .filter(|upload| upload.size.width == width && upload.size.height == height)
            .cloned()
            .collect()
    }

    /// The last upload of a `width`×`height` image.
    fn last_upload_of(&self, width: u32, height: u32) -> Upload {
        self.uploads_of(width, height)
            .pop()
            .unwrap_or_else(|| panic!("no {width}×{height} image was uploaded"))
    }

    /// Every texture made, in order.
    fn textures(&self) -> Vec<TextureId> {
        self.textures.lock().expect("the texture log").clone()
    }

    /// Every texture destroyed, in order.
    fn destroyed_textures(&self) -> Vec<TextureId> {
        self.destroyed_textures
            .lock()
            .expect("the texture log")
            .clone()
    }

    /// Every view destroyed, in order.
    fn destroyed_views(&self) -> Vec<TextureViewId> {
        self.destroyed_views.lock().expect("the view log").clone()
    }
}

impl GraphicsDevice for UploadLog {
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
        _data: &[u8],
    ) -> Result<BufferId, ResourceError> {
        Ok(BufferId(self.id()))
    }
    fn destroy_buffer(&self, _id: BufferId) -> Result<(), ResourceError> {
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
    fn create_texture(&self, _d: &TextureDescriptor) -> Result<TextureId, ResourceError> {
        let texture = TextureId(self.id());
        self.textures.lock().expect("the texture log").push(texture);
        Ok(texture)
    }
    fn destroy_texture(&self, id: TextureId) -> Result<(), ResourceError> {
        self.destroyed_textures
            .lock()
            .expect("the texture log")
            .push(id);
        Ok(())
    }
    fn write_texture(
        &self,
        texture_id: TextureId,
        data: &[u8],
        _bytes_per_row: Option<u32>,
        offset: Origin3D,
        size: Extent3D,
    ) -> Result<(), ResourceError> {
        self.uploads.lock().expect("the upload log").push(Upload {
            texture: texture_id,
            offset,
            size,
            bytes: data.to_vec(),
        });
        Ok(())
    }
    fn create_texture_view(
        &self,
        _texture_id: TextureId,
        _d: &TextureViewDescriptor,
    ) -> Result<TextureViewId, ResourceError> {
        Ok(TextureViewId(self.id()))
    }
    fn destroy_texture_view(&self, id: TextureViewId) -> Result<(), ResourceError> {
        self.destroyed_views.lock().expect("the view log").push(id);
        Ok(())
    }
    fn create_sampler(&self, _d: &SamplerDescriptor) -> Result<SamplerId, ResourceError> {
        Ok(SamplerId(self.id()))
    }
    fn destroy_sampler(&self, _id: SamplerId) -> Result<(), ResourceError> {
        Ok(())
    }
    fn create_command_encoder(&self, _label: Option<&str>) -> Box<dyn CommandEncoder> {
        unimplemented!("an upload records no command")
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

/// The registered data system called `name`.
fn data_system(name: &str) -> &'static DataSystemRegistration {
    inventory::iter::<DataSystemRegistration>
        .into_iter()
        .find(|system| system.name == name)
        .unwrap_or_else(|| panic!("`{name}` is not registered"))
}

/// A real 2×2 PNG of one colour.
fn png(rgba: [u8; 4]) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(2, 2, image::Rgba(rgba)))
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .expect("encodes a png");
    bytes
}

/// A project on disk whose assets hold one UI image, opened as the editor
/// opens one and served into a runtime with a device, an asset store, the UI
/// atlas and the reserve the `loading` family stages its decodes into.
struct Project {
    _dir: tempfile::TempDir,
    root: PathBuf,
    service: Arc<Mutex<AssetService>>,
    store: AssetStore,
    device: Arc<UploadLog>,
    atlas: Arc<UiImageAtlas>,
    reserve: Reserve,
    runtime: Runtime,
}

impl Project {
    fn with_image(bytes: &[u8]) -> Self {
        let dir = tempfile::tempdir().expect("a temp dir");
        let root = dir.path().to_path_buf();
        write_image(&root, bytes);
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
        let device = Arc::new(UploadLog::default());
        let atlas = Arc::new(UiImageAtlas::new());
        let reserve = Reserve::new();
        let mut runtime = Runtime::default();
        runtime.resources.insert(store.clone());
        runtime.resources.insert(Arc::clone(&atlas));
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
            atlas,
            reserve,
            runtime,
        }
    }

    /// One frame of the systems the image goes through: the boundary's
    /// intake of what was decoded off the frame, then the `PreExtract` reload,
    /// resolver and residency.
    fn frame(&self, world: &mut World) {
        let mut deck = OutputDeck::new();
        for name in [
            "asset_intake",
            "asset_reload",
            "asset_resolver",
            "image_on_gpu",
        ] {
            (data_system(name).run)(
                world,
                &self.runtime,
                &mut deck,
                khora_data::ecs::SystemContext {
                    since: khora_data::ecs::Tick(0),
                    resumed: false,
                    scope: None,
                },
            );
        }
    }

    /// One run of the `loading` family, played by hand: the decodes the frames
    /// filed, decoded through the service's `AssetDecoding` under their lease
    /// and received by the reserve. Returns how many it took.
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
    /// files no decode: what the frames asked for is decoded and taken in.
    fn settle(&self, world: &mut World) {
        for _ in 0..8 {
            self.frame(world);
            if self.decode_filed() == 0 {
                return;
            }
        }
        panic!("the frames still file decodes after 8 frames");
    }

    /// What the editor does when the image's file is saved again: the new
    /// bytes on disk, the asset announced as changed.
    fn save_image(&self, bytes: &[u8]) {
        write_image(&self.root, bytes);
        self.service
            .lock()
            .expect("the service")
            .invalidate(&AssetUUID::new_v5(IMAGE));
    }

    /// The pixels of the engine's current copy of the image.
    fn held_pixels(&self) -> Vec<u8> {
        self.store
            .store::<CpuTexture>()
            .read()
            .expect("the texture store")
            .get(&AssetUUID::new_v5(IMAGE))
            .map(|texture| texture.pixels.clone())
            .expect("the image is loaded")
    }
}

fn write_image(root: &Path, bytes: &[u8]) {
    let path = root.join("assets").join(IMAGE);
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("makes the folder");
    std::fs::write(path, bytes).expect("writes the image");
}

/// **An image changed on disk is uploaded again.** A UI image is uploaded
/// once it is decoded, and not again while its file stays the same; once the
/// file is saved with other pixels, the next frames upload it once more, with
/// the new pixels, and the atlas still places it.
#[test]
fn an_image_changed_on_disk_is_uploaded_again() {
    let project = Project::with_image(&png([255, 0, 0, 255]));
    let uuid = AssetUUID::new_v5(IMAGE);
    let mut world = World::new();
    world.spawn((
        UiTransform {
            pos: Vec2::new(0.0, 0.0),
            size: Vec2::new(32.0, 32.0),
            z_index: 0,
        },
        UiImage { texture: uuid },
    ));

    project.settle(&mut world);
    project.frame(&mut world);
    let first = project.device.uploads_of(2, 2);
    assert_eq!(
        first.len(),
        1,
        "the image is uploaded once, and not again while its file stays the same"
    );
    assert_eq!(first[0].bytes, project.held_pixels(), "the image's pixels");
    assert!(
        project.atlas.get_rect(&uuid).is_some(),
        "the atlas places the image"
    );

    project.save_image(&png([0, 255, 0, 255]));
    project.settle(&mut world);
    project.frame(&mut world);
    let all = project.device.uploads_of(2, 2);
    assert_eq!(all.len(), 2, "the changed image is uploaded once more");
    assert_eq!(
        all[1].bytes,
        project.held_pixels(),
        "the second upload carries the new pixels"
    );
    assert_ne!(all[0].bytes, all[1].bytes, "the file's pixels changed");
    assert!(
        project.atlas.get_rect(&uuid).is_some(),
        "the atlas still places the image"
    );
}

/// A decoded `width`×`height` RGBA image. Its pixels are only read once the
/// atlas found it room, so an image too big for the atlas carries none.
fn decoded(width: u32, height: u32, pixels: Vec<u8>) -> khora_core::asset::AssetHandle<CpuTexture> {
    use khora_core::renderer::api::resource::{SampleCount, TextureDimension, TextureUsage};
    khora_core::asset::AssetHandle::new(CpuTexture {
        pixels,
        size: Extent3D {
            width,
            height,
            depth_or_array_layers: 1,
        },
        format: TextureFormat::Rgba8Unorm,
        mip_level_count: 1,
        sample_count: SampleCount::X1,
        dimension: TextureDimension::D2,
        usage: TextureUsage::TEXTURE_BINDING | TextureUsage::COPY_DST,
    })
}

/// **An image the atlas refuses does not take the room of the ones that
/// fit.** A placed UI image whose file is replaced by one too big for the
/// atlas keeps its last good rect, and the residency tries the big copy again
/// every frame. Those refusals must cost the atlas nothing: an image that
/// fits, shown a while later, is still placed.
#[test]
fn a_refused_image_leaves_the_atlas_room_for_the_next() {
    let store = AssetStore::new();
    let device = Arc::new(UploadLog::default());
    let atlas = Arc::new(UiImageAtlas::new());
    let mut runtime = Runtime::default();
    runtime.resources.insert(store.clone());
    runtime.resources.insert(Arc::clone(&atlas));
    runtime
        .backends
        .insert(Arc::clone(&device) as Arc<dyn GraphicsDevice>);
    let residency = data_system("image_on_gpu");
    let frame = |world: &mut World| {
        let mut deck = OutputDeck::new();
        (residency.run)(
            world,
            &runtime,
            &mut deck,
            khora_data::ecs::SystemContext {
                since: khora_data::ecs::Tick(0),
                resumed: false,
                scope: None,
            },
        );
    };
    let transform = UiTransform {
        pos: Vec2::new(0.0, 0.0),
        size: Vec2::new(32.0, 32.0),
        z_index: 0,
    };

    let badge = AssetUUID::new_v5("ui/badge.png");
    let textures = store.store::<CpuTexture>();
    textures
        .write()
        .expect("the texture store")
        .insert(badge, decoded(2, 2, vec![255; 16]));
    let mut world = World::new();
    world.spawn((transform, UiImage { texture: badge }));
    frame(&mut world);
    assert!(atlas.get_rect(&badge).is_some(), "the badge is placed");

    // The badge's file is saved again, far bigger than the atlas.
    textures
        .write()
        .expect("the texture store")
        .insert(badge, decoded(4096, 4096, Vec::new()));
    for _ in 0..1100 {
        frame(&mut world);
    }
    assert!(
        atlas.get_rect(&badge).is_some(),
        "the refused badge keeps its last good rect"
    );

    let icon = AssetUUID::new_v5("ui/icon.png");
    textures
        .write()
        .expect("the texture store")
        .insert(icon, decoded(2, 2, vec![0; 16]));
    world.spawn((transform, UiImage { texture: icon }));
    frame(&mut world);
    assert!(
        atlas.get_rect(&icon).is_some(),
        "a 2×2 icon still fits an atlas that holds one 2×2 badge"
    );
}

/// **A changed image is drawn from its new rect.** Once the engine's copy of
/// a placed image is replaced, the rect the UI agent reads is the one the new
/// pixels were uploaded to, not the old one.
#[test]
fn a_replaced_image_moves_to_its_new_rect() {
    let store = AssetStore::new();
    let device = Arc::new(UploadLog::default());
    let atlas = Arc::new(UiImageAtlas::new());
    let mut runtime = Runtime::default();
    runtime.resources.insert(store.clone());
    runtime.resources.insert(Arc::clone(&atlas));
    runtime
        .backends
        .insert(Arc::clone(&device) as Arc<dyn GraphicsDevice>);
    let residency = data_system("image_on_gpu");
    let frame = |world: &mut World| {
        let mut deck = OutputDeck::new();
        (residency.run)(
            world,
            &runtime,
            &mut deck,
            khora_data::ecs::SystemContext {
                since: khora_data::ecs::Tick(0),
                resumed: false,
                scope: None,
            },
        );
    };

    let badge = AssetUUID::new_v5("ui/badge.png");
    let textures = store.store::<CpuTexture>();
    textures
        .write()
        .expect("the texture store")
        .insert(badge, decoded(2, 2, vec![255; 16]));
    let mut world = World::new();
    world.spawn((
        UiTransform {
            pos: Vec2::new(0.0, 0.0),
            size: Vec2::new(32.0, 32.0),
            z_index: 0,
        },
        UiImage { texture: badge },
    ));
    frame(&mut world);
    let first = atlas.get_rect(&badge).expect("the badge is placed");

    textures
        .write()
        .expect("the texture store")
        .insert(badge, decoded(2, 2, vec![0; 16]));
    frame(&mut world);
    frame(&mut world);
    let second = atlas.get_rect(&badge).expect("the badge is still placed");
    assert_ne!(first, second, "the new pixels sit in a new rect");
    assert_eq!(
        device.uploads_of(2, 2).len(),
        2,
        "the replaced copy is uploaded once, not every frame"
    );
}

/// The side of the atlas the rebuild tests fill: it holds four [`SIDE`]-pixel
/// images, two on each of two rows.
const SMALL_ATLAS: u32 = 64;

/// The side of an image of the rebuild tests.
const SIDE: u32 = 28;

/// The `image_on_gpu` residency alone, over a [`SMALL_ATLAS`], with the
/// images' copies put in the asset store by hand.
struct Residency {
    store: AssetStore,
    device: Arc<UploadLog>,
    atlas: Arc<UiImageAtlas>,
    runtime: Runtime,
    world: World,
    shown: Vec<(AssetUUID, EntityId)>,
}

impl Residency {
    fn new() -> Self {
        let store = AssetStore::new();
        let device = Arc::new(UploadLog::default());
        let atlas = Arc::new(UiImageAtlas::with_size(SMALL_ATLAS));
        let mut runtime = Runtime::default();
        runtime.resources.insert(store.clone());
        runtime.resources.insert(Arc::clone(&atlas));
        runtime
            .backends
            .insert(Arc::clone(&device) as Arc<dyn GraphicsDevice>);
        Self {
            store,
            device,
            atlas,
            runtime,
            world: World::new(),
            shown: Vec::new(),
        }
    }

    /// Puts a new copy of the image `name` in the asset store, every pixel
    /// byte `fill`; returns its uuid.
    fn copy(&self, name: &str, fill: u8) -> AssetUUID {
        self.copy_sized(name, SIDE, vec![fill; (SIDE * SIDE * 4) as usize])
    }

    /// Puts a new `side`-pixel copy of the image `name` in the asset store;
    /// returns its uuid.
    fn copy_sized(&self, name: &str, side: u32, pixels: Vec<u8>) -> AssetUUID {
        let uuid = AssetUUID::new_v5(name);
        self.store
            .store::<CpuTexture>()
            .write()
            .expect("the texture store")
            .insert(uuid, decoded(side, side, pixels));
        uuid
    }

    /// Shows the image `name`, every pixel byte `fill`, on an entity of its
    /// own; returns its uuid.
    fn show(&mut self, name: &str, fill: u8) -> AssetUUID {
        let uuid = self.copy(name, fill);
        self.name(uuid);
        uuid
    }

    /// Spawns an entity whose `UiImage` names `image`.
    fn name(&mut self, image: AssetUUID) {
        let entity = self.world.spawn((
            UiTransform {
                pos: Vec2::new(0.0, 0.0),
                size: Vec2::new(32.0, 32.0),
                z_index: 0,
            },
            UiImage { texture: image },
        ));
        self.shown.push((image, entity));
    }

    /// Stops showing `image`: its entity is despawned.
    fn hide(&mut self, image: AssetUUID) {
        let at = self
            .shown
            .iter()
            .position(|(uuid, _)| *uuid == image)
            .expect("the image is shown");
        let (_, entity) = self.shown.remove(at);
        assert!(self.world.despawn(entity), "the entity was alive");
    }

    /// One run of the residency.
    fn frame(&mut self) {
        let mut deck = OutputDeck::new();
        (data_system("image_on_gpu").run)(
            &mut self.world,
            &self.runtime,
            &mut deck,
            khora_data::ecs::SystemContext {
                since: khora_data::ecs::Tick(0),
                resumed: false,
                scope: None,
            },
        );
    }

    /// The atlas's texture and view now.
    fn atlas_texture(&self) -> (TextureId, TextureViewId) {
        let guard = self.atlas.lock_atlas().expect("the atlas lock");
        let atlas = guard.as_ref().expect("the atlas is made");
        (atlas.texture(), atlas.view())
    }

    /// Asserts that `image` is drawn from a rect of the atlas's texture now,
    /// where pixels all `fill` were uploaded.
    fn assert_drawn(&self, image: AssetUUID, fill: u8, what: &str) {
        let rect = self
            .atlas
            .get_rect(&image)
            .unwrap_or_else(|| panic!("{what}: the image has a rect"));
        let (texture, _) = self.atlas_texture();
        let at = Origin3D {
            x: (rect.min.x * SMALL_ATLAS as f32).round() as u32,
            y: (rect.min.y * SMALL_ATLAS as f32).round() as u32,
            z: 0,
        };
        let upload = self
            .device
            .uploads_of(SIDE, SIDE)
            .into_iter()
            .rev()
            .find(|upload| upload.texture == texture && upload.offset == at)
            .unwrap_or_else(|| panic!("{what}: nothing was uploaded at its rect"));
        assert!(
            upload.bytes.iter().all(|byte| *byte == fill),
            "{what}: its rect holds the pixels {}, not {fill}",
            upload.bytes[0]
        );
    }
}

/// **Replacing an image again and again never fills the atlas.** Each copy of
/// a shown image is placed beside the last, and the rects of the copies it
/// replaced are no use to anyone: the atlas is built again from the images in
/// use when it runs out of room, so every new copy is drawn.
#[test]
fn replacing_an_image_again_and_again_never_fills_the_atlas() {
    let mut residency = Residency::new();
    let badge = residency.show("ui/badge.png", 0);
    residency.frame();
    residency.assert_drawn(badge, 0, "the first copy");

    for fill in 1..=20 {
        residency.copy("ui/badge.png", fill);
        residency.frame();
        residency.assert_drawn(badge, fill, &format!("copy {fill}"));
    }
}

/// **An image the full atlas had no room for fits once a rebuild drops the
/// images hidden.** The atlas holds four images, three of which are
/// then hidden; a fifth, shown next, is placed in an atlas built again from
/// the two images in use, and the hidden ones keep no rect.
#[test]
fn an_image_that_did_not_fit_fits_once_a_rebuild_drops_the_unused_ones() {
    let mut residency = Residency::new();
    let a = residency.show("ui/a.png", 1);
    let b = residency.show("ui/b.png", 2);
    let c = residency.show("ui/c.png", 3);
    let d = residency.show("ui/d.png", 4);
    residency.frame();
    for (image, fill) in [(a, 1), (b, 2), (c, 3), (d, 4)] {
        residency.assert_drawn(image, fill, "an image of the full atlas");
    }

    residency.hide(a);
    residency.hide(b);
    residency.hide(c);
    let e = residency.show("ui/e.png", 5);
    residency.frame();

    residency.assert_drawn(e, 5, "the image the full atlas refused");
    residency.assert_drawn(d, 4, "the image still shown");
    for hidden in [a, b, c] {
        assert!(
            residency.atlas.get_rect(&hidden).is_none(),
            "an image no longer shown keeps no rect in the rebuilt atlas"
        );
    }
}

/// **The images in use keep drawing, at their new rects, after a rebuild.**
/// The rebuild uploads each image in use again, from its copy in the asset
/// store, into a new texture; the old texture and its view are destroyed, and
/// each rect points where its pixels now are.
#[test]
fn images_in_use_keep_drawing_at_their_new_rects_after_a_rebuild() {
    let mut residency = Residency::new();
    let a = residency.show("ui/a.png", 1);
    let b = residency.show("ui/b.png", 2);
    let c = residency.show("ui/c.png", 3);
    let d = residency.show("ui/d.png", 4);
    residency.frame();
    let (old_texture, old_view) = residency.atlas_texture();

    residency.hide(a);
    let e = residency.show("ui/e.png", 5);
    residency.frame();

    assert_eq!(
        residency.device.textures().len(),
        2,
        "the full atlas was built again"
    );
    let (texture, view) = residency.atlas_texture();
    assert_ne!(texture, old_texture, "the rebuilt atlas is a new texture");
    assert_ne!(view, old_view, "with a new view, which the UI lane binds");
    assert_eq!(
        residency.device.destroyed_textures(),
        vec![old_texture],
        "the old texture is destroyed"
    );
    assert_eq!(
        residency.device.destroyed_views(),
        vec![old_view],
        "and its view"
    );
    let mut rects = Vec::new();
    for (image, fill) in [(b, 2), (c, 3), (d, 4), (e, 5)] {
        residency.assert_drawn(image, fill, "an image in use after the rebuild");
        let rect = residency.atlas.get_rect(&image).expect("placed");
        assert!(!rects.contains(&rect), "each image at a rect of its own");
        rects.push(rect);
    }
    assert_eq!(
        residency.device.last_upload_of(SIDE, SIDE).texture,
        texture,
        "nothing is uploaded into the old texture after it is destroyed"
    );
}

/// **A full atlas is built again once a run, however many images it
/// refuses**, and not again while every image in use stays: an image left out
/// waits until one in use goes away, then is placed.
#[test]
fn a_full_atlas_is_rebuilt_once_a_run_however_many_images_it_refuses() {
    let mut residency = Residency::new();
    let a = residency.show("ui/a.png", 1);
    residency.show("ui/b.png", 2);
    residency.show("ui/c.png", 3);
    residency.show("ui/d.png", 4);
    residency.frame();
    assert_eq!(residency.device.textures().len(), 1, "one atlas made");

    let e = residency.show("ui/e.png", 5);
    let f = residency.show("ui/f.png", 6);
    let g = residency.show("ui/g.png", 7);
    residency.frame();
    assert_eq!(
        residency.device.textures().len(),
        2,
        "three images refused in one run rebuild the atlas once"
    );
    for refused in [e, f, g] {
        assert!(
            residency.atlas.get_rect(&refused).is_none(),
            "four images in use fill the rebuilt atlas: the new ones are left out"
        );
    }

    for _ in 0..3 {
        residency.frame();
    }
    assert_eq!(
        residency.device.textures().len(),
        2,
        "no rebuild while every image in use stays"
    );

    residency.hide(a);
    residency.frame();
    assert_eq!(
        residency.device.textures().len(),
        3,
        "an image in use going away rebuilds the atlas for those left out"
    );
    let placed: Vec<(AssetUUID, u8)> = [(e, 5), (f, 6), (g, 7)]
        .into_iter()
        .filter(|(image, _)| residency.atlas.get_rect(image).is_some())
        .collect();
    assert_eq!(
        placed.len(),
        1,
        "the room of the image gone takes one of those left out"
    );
    residency.assert_drawn(
        placed[0].0,
        placed[0].1,
        "an image left out, once there is room",
    );
}

/// **A refused copy keeps its last good one drawn across a rebuild.** An image
/// whose new copy is larger than the atlas keeps the copy it last had placed;
/// the rebuild uploads that copy again, so the image is not lost.
#[test]
fn a_refused_copy_keeps_its_last_good_one_drawn_across_a_rebuild() {
    let mut residency = Residency::new();
    let badge = residency.show("ui/badge.png", 1);
    let b = residency.show("ui/b.png", 2);
    residency.show("ui/c.png", 3);
    residency.show("ui/d.png", 4);
    residency.frame();

    residency.copy_sized("ui/badge.png", SMALL_ATLAS * 2, Vec::new());
    residency.frame();
    residency.assert_drawn(badge, 1, "the last good copy, before the rebuild");

    residency.hide(b);
    let e = residency.show("ui/e.png", 5);
    residency.frame();
    assert_eq!(
        residency.device.textures().len(),
        2,
        "the atlas was rebuilt"
    );
    residency.assert_drawn(badge, 1, "the last good copy, after the rebuild");
    residency.assert_drawn(e, 5, "the image that made the rebuild");
}

/// **An image larger than the atlas rebuilds nothing**: no rebuild could make
/// room for it, so the images in use keep their rects in the same texture.
#[test]
fn an_image_larger_than_the_atlas_rebuilds_nothing() {
    let mut residency = Residency::new();
    let a = residency.show("ui/a.png", 1);
    residency.show("ui/b.png", 2);
    residency.show("ui/c.png", 3);
    residency.show("ui/d.png", 4);
    residency.frame();

    let huge = residency.copy_sized("ui/huge.png", SMALL_ATLAS * 2, Vec::new());
    residency.name(huge);
    for _ in 0..3 {
        residency.frame();
    }
    assert_eq!(residency.device.textures().len(), 1, "no rebuild");
    assert!(
        residency.atlas.get_rect(&huge).is_none(),
        "the huge one is refused"
    );
    residency.assert_drawn(a, 1, "an image in use");
}

/// **A refused copy's last good one, left out by a rebuild, is placed once
/// there is room.** The badge's new copy is larger than the atlas, so its last
/// good copy is drawn; a rebuild with no room for that copy leaves it out, and
/// once the image that filled the atlas goes away, the badge is drawn again.
#[test]
fn a_last_good_copy_left_out_by_a_rebuild_is_placed_once_there_is_room() {
    let mut residency = Residency::new();
    let filler = residency.show("ui/filler.png", 2);
    let badge = residency.show("ui/badge.png", 1);
    residency.frame();
    residency.copy_sized("ui/badge.png", SMALL_ATLAS * 2, Vec::new());
    residency.frame();
    residency.assert_drawn(badge, 1, "the last good copy, before the rebuild");

    // The filler's new copy takes the whole atlas: the rebuild has no room
    // left for the badge's last good copy.
    let whole = SMALL_ATLAS - 4;
    residency.copy_sized(
        "ui/filler.png",
        whole,
        vec![3; (whole * whole * 4) as usize],
    );
    residency.frame();
    assert!(
        residency.atlas.get_rect(&filler).is_some(),
        "the filler's new copy is placed"
    );

    residency.hide(filler);
    for _ in 0..3 {
        residency.frame();
    }
    residency.assert_drawn(badge, 1, "the last good copy, once the filler is gone");
}

/// **An image whose new copy is still being decoded keeps its rect across a
/// rebuild.** A hot reload drops the engine's copy until the new one is
/// decoded; meanwhile the image is still shown, and the atlas drew its last
/// copy. A rebuild in that window must place that copy again, not drop it.
#[test]
fn an_image_being_reloaded_keeps_its_rect_across_a_rebuild() {
    let mut residency = Residency::new();
    let a = residency.show("ui/a.png", 1);
    let b = residency.show("ui/b.png", 2);
    residency.show("ui/c.png", 3);
    residency.show("ui/d.png", 4);
    residency.frame();
    residency.assert_drawn(a, 1, "before its file changes");

    // Its file changed: the engine's copy is dropped until the new one lands.
    residency
        .store
        .store::<CpuTexture>()
        .write()
        .expect("the texture store")
        .remove(&a);
    residency.hide(b);
    let e = residency.show("ui/e.png", 5);
    residency.frame();
    assert_eq!(
        residency.device.textures().len(),
        2,
        "the atlas was rebuilt"
    );
    residency.assert_drawn(e, 5, "the image that made the rebuild");
    residency.assert_drawn(a, 1, "the image being reloaded, from its last copy");
}

/// **A reload while images are left out rebuilds nothing.** While a rebuilt
/// atlas leaves an image out, a placed image and the image left out both lose
/// their copy to a reload: each still draws its last good copy, so the images
/// in use draw what they drew, and the atlas is not built again until a new
/// copy lands.
#[test]
fn a_reload_while_images_are_left_out_rebuilds_nothing() {
    let mut residency = Residency::new();
    let a = residency.show("ui/a.png", 1);
    residency.show("ui/b.png", 2);
    residency.show("ui/c.png", 3);
    residency.show("ui/d.png", 4);
    residency.frame();
    let e = residency.show("ui/e.png", 5);
    residency.frame();
    assert_eq!(
        residency.device.textures().len(),
        2,
        "the atlas was rebuilt"
    );
    assert!(
        residency.atlas.get_rect(&e).is_none(),
        "the fifth is left out"
    );

    for reloading in [a, e] {
        residency
            .store
            .store::<CpuTexture>()
            .write()
            .expect("the texture store")
            .remove(&reloading);
    }
    for _ in 0..3 {
        residency.frame();
    }
    assert_eq!(
        residency.device.textures().len(),
        2,
        "a reload in flight changes no copy drawn: no rebuild"
    );
    residency.assert_drawn(a, 1, "the placed image, from its last good copy");

    residency.copy("ui/a.png", 9);
    residency.frame();
    assert_eq!(
        residency.device.textures().len(),
        3,
        "the new copy landing rebuilds the atlas for the image left out"
    );
    residency.assert_drawn(a, 9, "the placed image, from its new copy");
}

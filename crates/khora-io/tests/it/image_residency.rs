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

use khora_core::asset::AssetUUID;
use khora_core::lane::OutputDeck;
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

/// One texture upload: its size and its bytes.
#[derive(Debug, Clone)]
struct Upload {
    size: Extent3D,
    bytes: Vec<u8>,
}

/// A device that creates whatever it is asked to and records every texture
/// upload.
#[derive(Debug, Default)]
struct UploadLog {
    next_id: AtomicUsize,
    uploads: Mutex<Vec<Upload>>,
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
        Ok(TextureId(self.id()))
    }
    fn destroy_texture(&self, _id: TextureId) -> Result<(), ResourceError> {
        Ok(())
    }
    fn write_texture(
        &self,
        _texture_id: TextureId,
        data: &[u8],
        _bytes_per_row: Option<u32>,
        _offset: Origin3D,
        size: Extent3D,
    ) -> Result<(), ResourceError> {
        self.uploads.lock().expect("the upload log").push(Upload {
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
/// opens one and served into a runtime with a device, an asset store and the
/// UI atlas.
struct Project {
    _dir: tempfile::TempDir,
    root: PathBuf,
    service: Arc<Mutex<AssetService>>,
    store: AssetStore,
    device: Arc<UploadLog>,
    atlas: Arc<UiImageAtlas>,
    runtime: Runtime,
}

impl Project {
    fn with_image(bytes: &[u8]) -> Self {
        let dir = tempfile::tempdir().expect("a temp dir");
        let root = dir.path().to_path_buf();
        write_image(&root, bytes);
        let service = AssetService::open_loose_files(
            &root,
            &AssetIdRegistry::load(&root),
            Arc::new(MetricsRegistry::new()),
        )
        .expect("the project opens");
        let service = Arc::new(Mutex::new(service));
        let store = AssetStore::new();
        let device = Arc::new(UploadLog::default());
        let atlas = Arc::new(UiImageAtlas::new());
        let mut runtime = Runtime::default();
        runtime.resources.insert(store.clone());
        runtime.resources.insert(Arc::clone(&atlas));
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
            runtime,
        }
    }

    /// One frame of the PreExtract systems the image goes through: the
    /// reload, the resolver, then the residency.
    fn frame(&self, world: &mut World) {
        let mut deck = OutputDeck::new();
        for name in ["asset_reload", "asset_resolver", "image_on_gpu"] {
            (data_system(name).run)(
                world,
                &self.runtime,
                &mut deck,
                khora_data::ecs::SystemContext {
                    since: khora_data::ecs::Tick(0),
                },
            );
        }
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
/// once, and not again while its file stays the same; once the file is saved
/// with other pixels, the next frames upload it once more, with the new
/// pixels, and the atlas still places it.
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

    project.frame(&mut world);
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
    project.frame(&mut world);
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

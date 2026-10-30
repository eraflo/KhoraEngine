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

//! The `glyphs_on_gpu` DataSystem, run against a device that records the
//! textures it makes, what is uploaded into them and what is destroyed, and a
//! rasteriser that records what it is asked.

use std::sync::{Arc, Mutex, MutexGuard};

use khora_core::asset::font::Font;
use khora_core::asset::{AssetHandle, AssetUUID};
use khora_core::ecs::entity::EntityId;
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
    BufferDescriptor, BufferId, SamplerDescriptor, SamplerId, TextureDescriptor, TextureFormat,
    TextureId, TextureViewDescriptor, TextureViewId,
};
use khora_core::renderer::api::shader::{ShaderModuleDescriptor, ShaderModuleId};
use khora_core::renderer::error::ResourceError;
use khora_core::renderer::traits::{CommandEncoder, GraphicsDevice};
use khora_core::ui::text::{
    FontError, GlyphImage, GlyphKey, GlyphKind, GlyphRasterizer, ShapedGlyph,
};
use khora_core::Runtime;

use super::glyphs_on_gpu;
use crate::ecs::{
    ComponentKey, DataSystemRegistration, HandleComponent, SystemContext, Tick, TickPhase, World,
};
use crate::ui::{GlyphAtlas, UiTextLayout, UiTransform};

/// A font asset the tests load.
const BODY: AssetUUID = AssetUUID::from_bytes(*b"khora:test:body!");

/// The size every text is laid out at, unless a test says otherwise.
const SIZE: f32 = 16.0;

/// The glyph the fake rasteriser draws nothing for: a space.
const SPACE: u16 = b' ' as u16;

/// Glyph ids from this one up are colour glyphs (an emoji) to the fake
/// rasteriser; every other glyph but [`SPACE`] is a mask.
const COLOR_GLYPHS: u16 = 1000;

/// What the fake rasteriser makes of a mask glyph: a 5 × 8 coverage mask.
const MASK_WIDTH: u32 = 5;
const MASK_HEIGHT: u32 = 8;
const MASK_LEFT: i32 = 1;
const MASK_TOP: i32 = 7;

/// What it makes of a colour glyph: a 6 × 6 RGBA image.
const COLOR_WIDTH: u32 = 6;
const COLOR_HEIGHT: u32 = 6;
const COLOR_LEFT: i32 = -1;
const COLOR_TOP: i32 = 9;

/// The side of the atlases the tests that fill one use. Its shelves (a 2 px
/// padding, starting 2 px in) hold 4 masks a row on 3 rows — 12 masks — and
/// 3 colour glyphs a row on 3 rows — 9 colour glyphs.
const TINY: u32 = 32;
/// How many masks a [`TINY`] atlas holds.
const TINY_MASKS: usize = 12;

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

// --- the fake rasteriser ---

/// A `GlyphRasterizer` that records every call. A [`SPACE`] draws nothing;
/// a glyph from [`COLOR_GLYPHS`] up is a colour image; any other a mask.
#[derive(Default)]
struct FakeRasterizer {
    asked: Mutex<Vec<GlyphKey>>,
    registered: Mutex<Vec<(AssetUUID, Vec<u8>)>>,
}

impl FakeRasterizer {
    /// Every key it was asked to rasterise, in call order.
    fn asked(&self) -> Vec<GlyphKey> {
        locked(&self.asked).clone()
    }

    /// How many times it was asked to rasterise `key`.
    fn times_asked(&self, key: &GlyphKey) -> usize {
        locked(&self.asked).iter().filter(|k| *k == key).count()
    }

    /// Every registration, in call order.
    fn registrations(&self) -> Vec<(AssetUUID, Vec<u8>)> {
        locked(&self.registered).clone()
    }
}

impl GlyphRasterizer for FakeRasterizer {
    fn register_font(&self, id: AssetUUID, data: Arc<[u8]>) -> Result<(), FontError> {
        locked(&self.registered).push((id, data.to_vec()));
        Ok(())
    }

    fn rasterize(&self, key: &GlyphKey) -> Option<GlyphImage> {
        locked(&self.asked).push(*key);
        if key.glyph == SPACE {
            None
        } else if key.glyph >= COLOR_GLYPHS {
            Some(GlyphImage {
                width: COLOR_WIDTH,
                height: COLOR_HEIGHT,
                left: COLOR_LEFT,
                top: COLOR_TOP,
                kind: GlyphKind::Color,
                pixels: vec![200; (COLOR_WIDTH * COLOR_HEIGHT * 4) as usize],
            })
        } else {
            Some(GlyphImage {
                width: MASK_WIDTH,
                height: MASK_HEIGHT,
                left: MASK_LEFT,
                top: MASK_TOP,
                kind: GlyphKind::Mask,
                pixels: vec![255; (MASK_WIDTH * MASK_HEIGHT) as usize],
            })
        }
    }
}

// --- the recording device ---

/// One texture the device made.
#[derive(Debug, Clone, Copy)]
struct MadeTexture {
    id: TextureId,
    format: TextureFormat,
    size: Extent3D,
}

/// One `write_texture` the device received.
#[derive(Debug, Clone, Copy)]
struct Upload {
    texture: TextureId,
    width: u32,
    height: u32,
    bytes: usize,
}

#[derive(Debug, Default)]
struct DeviceLog {
    next_id: usize,
    textures: Vec<MadeTexture>,
    views: Vec<(TextureViewId, TextureId)>,
    uploads: Vec<Upload>,
    destroyed_textures: Vec<TextureId>,
    destroyed_views: Vec<TextureViewId>,
}

impl DeviceLog {
    fn next(&mut self) -> usize {
        self.next_id += 1;
        self.next_id
    }
}

/// A device that makes every texture and view it is asked for and records
/// them, every upload and every destruction.
#[derive(Debug, Default)]
struct GlyphDevice {
    log: Mutex<DeviceLog>,
}

impl GlyphDevice {
    fn log(&self) -> MutexGuard<'_, DeviceLog> {
        locked(&self.log)
    }

    fn textures(&self) -> Vec<MadeTexture> {
        self.log().textures.clone()
    }

    /// The textures made in an RGBA format.
    fn rgba_textures(&self) -> Vec<MadeTexture> {
        self.textures()
            .into_iter()
            .filter(|t| is_rgba(t.format))
            .collect()
    }

    /// The textures made in R8.
    fn r8_textures(&self) -> Vec<MadeTexture> {
        self.textures()
            .into_iter()
            .filter(|t| t.format == TextureFormat::R8Unorm)
            .collect()
    }

    fn texture(&self, id: TextureId) -> Option<MadeTexture> {
        self.textures().into_iter().find(|t| t.id == id)
    }

    /// The texture `view` was made on.
    fn texture_of_view(&self, view: TextureViewId) -> Option<TextureId> {
        self.log()
            .views
            .iter()
            .find(|(v, _)| *v == view)
            .map(|(_, texture)| *texture)
    }

    /// The uploads into `texture` smaller than the texture itself: the
    /// glyphs, without the clear an atlas is made with.
    fn glyph_uploads_to(&self, texture: TextureId) -> Vec<Upload> {
        let Some(made) = self.texture(texture) else {
            return Vec::new();
        };
        self.log()
            .uploads
            .iter()
            .filter(|u| u.texture == texture)
            .filter(|u| (u.width, u.height) != (made.size.width, made.size.height))
            .copied()
            .collect()
    }

    fn upload_count(&self) -> usize {
        self.log().uploads.len()
    }

    fn destroyed_textures(&self) -> Vec<TextureId> {
        self.log().destroyed_textures.clone()
    }

    fn destroyed_views(&self) -> Vec<TextureViewId> {
        self.log().destroyed_views.clone()
    }
}

fn is_rgba(format: TextureFormat) -> bool {
    matches!(
        format,
        TextureFormat::Rgba8Unorm | TextureFormat::Rgba8UnormSrgb
    )
}

impl GraphicsDevice for GlyphDevice {
    fn create_shader_module(
        &self,
        _d: &ShaderModuleDescriptor,
    ) -> Result<ShaderModuleId, ResourceError> {
        unreachable!("placing glyphs compiles no shader")
    }
    fn destroy_shader_module(&self, _id: ShaderModuleId) -> Result<(), ResourceError> {
        unreachable!("placing glyphs compiles no shader")
    }
    fn create_render_pipeline(
        &self,
        _d: &RenderPipelineDescriptor,
    ) -> Result<RenderPipelineId, ResourceError> {
        unreachable!("placing glyphs builds no pipeline")
    }
    fn create_pipeline_layout(
        &self,
        _d: &PipelineLayoutDescriptor,
    ) -> Result<PipelineLayoutId, ResourceError> {
        unreachable!("placing glyphs builds no pipeline")
    }
    fn destroy_render_pipeline(&self, _id: RenderPipelineId) -> Result<(), ResourceError> {
        unreachable!("placing glyphs builds no pipeline")
    }
    fn create_compute_pipeline(
        &self,
        _d: &ComputePipelineDescriptor,
    ) -> Result<ComputePipelineId, ResourceError> {
        unreachable!("placing glyphs builds no pipeline")
    }
    fn destroy_compute_pipeline(&self, _id: ComputePipelineId) -> Result<(), ResourceError> {
        unreachable!("placing glyphs builds no pipeline")
    }
    fn create_bind_group_layout(
        &self,
        _d: &BindGroupLayoutDescriptor,
    ) -> Result<BindGroupLayoutId, ResourceError> {
        unreachable!("placing glyphs binds nothing")
    }
    fn create_bind_group(&self, _d: &BindGroupDescriptor) -> Result<BindGroupId, ResourceError> {
        unreachable!("placing glyphs binds nothing")
    }
    fn destroy_bind_group_layout(&self, _id: BindGroupLayoutId) -> Result<(), ResourceError> {
        unreachable!("placing glyphs binds nothing")
    }
    fn destroy_bind_group(&self, _id: BindGroupId) -> Result<(), ResourceError> {
        unreachable!("placing glyphs binds nothing")
    }
    fn create_buffer(&self, _d: &BufferDescriptor) -> Result<BufferId, ResourceError> {
        unreachable!("placing glyphs makes no buffer")
    }
    fn create_buffer_with_data(
        &self,
        _d: &BufferDescriptor,
        _data: &[u8],
    ) -> Result<BufferId, ResourceError> {
        unreachable!("placing glyphs makes no buffer")
    }
    fn destroy_buffer(&self, _id: BufferId) -> Result<(), ResourceError> {
        unreachable!("placing glyphs makes no buffer")
    }
    fn write_buffer(&self, _id: BufferId, _offset: u64, _data: &[u8]) -> Result<(), ResourceError> {
        unreachable!("placing glyphs makes no buffer")
    }
    fn write_buffer_async<'a>(
        &'a self,
        _id: BufferId,
        _offset: u64,
        _data: &'a [u8],
    ) -> Box<dyn std::future::Future<Output = Result<(), ResourceError>> + Send + 'static> {
        unreachable!("placing glyphs makes no buffer")
    }
    fn create_texture(&self, d: &TextureDescriptor) -> Result<TextureId, ResourceError> {
        let mut log = self.log();
        let id = TextureId(log.next());
        log.textures.push(MadeTexture {
            id,
            format: d.format,
            size: d.size,
        });
        Ok(id)
    }
    fn destroy_texture(&self, id: TextureId) -> Result<(), ResourceError> {
        self.log().destroyed_textures.push(id);
        Ok(())
    }
    fn write_texture(
        &self,
        id: TextureId,
        data: &[u8],
        _bytes_per_row: Option<u32>,
        _offset: Origin3D,
        size: Extent3D,
    ) -> Result<(), ResourceError> {
        self.log().uploads.push(Upload {
            texture: id,
            width: size.width,
            height: size.height,
            bytes: data.len(),
        });
        Ok(())
    }
    fn create_texture_view(
        &self,
        texture: TextureId,
        _d: &TextureViewDescriptor,
    ) -> Result<TextureViewId, ResourceError> {
        let mut log = self.log();
        let id = TextureViewId(log.next());
        log.views.push((id, texture));
        Ok(id)
    }
    fn destroy_texture_view(&self, id: TextureViewId) -> Result<(), ResourceError> {
        self.log().destroyed_views.push(id);
        Ok(())
    }
    fn create_sampler(&self, _d: &SamplerDescriptor) -> Result<SamplerId, ResourceError> {
        unreachable!("placing glyphs samples nothing")
    }
    fn destroy_sampler(&self, _id: SamplerId) -> Result<(), ResourceError> {
        unreachable!("placing glyphs samples nothing")
    }
    fn create_command_encoder(&self, _label: Option<&str>) -> Box<dyn CommandEncoder> {
        unreachable!("placing glyphs records no command")
    }
    fn submit_command_buffer(&self, _command_buffer: CommandBufferId) {
        unreachable!("placing glyphs records no command")
    }
    fn get_surface_format(&self) -> Option<TextureFormat> {
        None
    }
    fn get_surface_size(&self) -> (u32, u32) {
        (0, 0)
    }
    fn get_adapter_info(&self) -> GraphicsAdapterInfo {
        unreachable!("placing glyphs reads no adapter")
    }
    fn supports_feature(&self, _feature_name: &str) -> bool {
        false
    }
}

// --- the scene ---

/// Which of the system's three runtime entries a scene holds.
struct Parts {
    device: bool,
    rasterizer: bool,
    atlas: bool,
}

const ALL: Parts = Parts {
    device: true,
    rasterizer: true,
    atlas: true,
};

/// A world, a runtime and the system's last-seen tick: each `run` is one run
/// of the system as the runner makes it, handed the tick of its previous run.
struct Scene {
    world: World,
    runtime: Runtime,
    device: Arc<GlyphDevice>,
    rasterizer: Arc<FakeRasterizer>,
    atlas: Arc<GlyphAtlas>,
    last_seen: Tick,
}

impl Scene {
    /// A scene with every entry, its atlas of the default size.
    fn new() -> Self {
        Self::build(GlyphAtlas::default(), ALL)
    }

    /// A scene with every entry, its atlases `size` pixels square.
    fn with_atlas_size(size: u32) -> Self {
        Self::build(GlyphAtlas::new(size), ALL)
    }

    /// A scene whose runtime holds only the `parts` asked for. The device,
    /// the rasteriser and the atlas are kept to be read either way.
    fn build(atlas: GlyphAtlas, parts: Parts) -> Self {
        let device = Arc::new(GlyphDevice::default());
        let rasterizer = Arc::new(FakeRasterizer::default());
        let atlas = Arc::new(atlas);
        let mut runtime = Runtime::new();
        if parts.device {
            let backend: Arc<dyn GraphicsDevice> = device.clone();
            runtime.backends.insert(backend);
        }
        if parts.rasterizer {
            let backend: Arc<dyn GlyphRasterizer> = rasterizer.clone();
            runtime.backends.insert(backend);
        }
        if parts.atlas {
            runtime.resources.insert(atlas.clone());
        }
        Self {
            world: World::new(),
            runtime,
            device,
            rasterizer,
            atlas,
            last_seen: Tick(0),
        }
    }

    /// Spawns a laid-out text in `font`, with no font handle.
    fn text(&mut self, font: AssetUUID, glyphs: &[(u16, f32)]) -> EntityId {
        self.world
            .spawn((UiTransform::default(), layout(font, SIZE, glyphs)))
    }

    /// Spawns a laid-out text holding `handle`, in the font it names.
    fn text_with_handle(
        &mut self,
        handle: &HandleComponent<Font>,
        glyphs: &[(u16, f32)],
    ) -> EntityId {
        self.world.spawn((
            UiTransform::default(),
            layout(handle.uuid, SIZE, glyphs),
            handle.clone(),
        ))
    }

    /// Lays `entity`'s text out again, as `ui_text_layout` does when its text
    /// or its font changes.
    fn relayout(&mut self, entity: EntityId, font: AssetUUID, glyphs: &[(u16, f32)]) {
        assert!(
            self.world.set_component(entity, layout(font, SIZE, glyphs)),
            "{entity:?} has a layout to replace"
        );
    }

    /// One run of the system.
    fn run(&mut self) {
        let tick = self.world.begin_run();
        glyphs_on_gpu(
            &mut self.world,
            &self.runtime,
            &mut OutputDeck::new(),
            SystemContext {
                since: self.last_seen,
            },
        );
        self.world.end_run();
        self.last_seen = tick;
    }

    /// The texture behind the mask atlas's view, once there is one.
    fn mask_texture(&self) -> Option<TextureId> {
        self.atlas
            .views()
            .0
            .and_then(|view| self.device.texture_of_view(view))
    }

    /// The texture behind the colour atlas's view, once there is one.
    fn color_texture(&self) -> Option<TextureId> {
        self.atlas
            .views()
            .1
            .and_then(|view| self.device.texture_of_view(view))
    }
}

/// A layout of `glyphs` (`(id, x)`), every glyph on one baseline.
fn layout(font: AssetUUID, size_px: f32, glyphs: &[(u16, f32)]) -> UiTextLayout {
    UiTextLayout {
        font,
        size_px,
        size: Vec2::new(64.0, size_px),
        lines: 1,
        glyphs: glyphs
            .iter()
            .map(|&(id, x)| ShapedGlyph { id, x, y: size_px })
            .collect(),
    }
}

/// The glyphs of `text`, one per character, its id the character's code,
/// pens 8 px apart from 0 — whole pixels, so every glyph in bin 0.
fn chars(text: &str) -> Vec<(u16, f32)> {
    text.chars()
        .enumerate()
        .map(|(i, c)| (c as u16, i as f32 * 8.0))
        .collect()
}

/// The glyphs `ids`, pens 8 px apart from 0.
fn ids(ids: impl IntoIterator<Item = u16>) -> Vec<(u16, f32)> {
    ids.into_iter()
        .enumerate()
        .map(|(i, id)| (id, i as f32 * 8.0))
        .collect()
}

/// The key of glyph `id` at `SIZE` with its pen at a whole pixel.
fn key(font: AssetUUID, id: u16) -> GlyphKey {
    GlyphKey::new(font, id, SIZE, 0.0)
}

fn font_handle(uuid: AssetUUID, data: &[u8]) -> HandleComponent<Font> {
    HandleComponent {
        handle: AssetHandle::new(Font {
            name: "Body".to_owned(),
            data: data.to_vec(),
        }),
        uuid,
    }
}

// --- resident after one run ---

/// **Every glyph of a layout is resident after one run**, each with a slot.
#[test]
fn every_glyph_of_a_layout_is_resident_after_one_run() {
    let mut scene = Scene::new();
    scene.text(Font::BUILT_IN, &chars("Hi!"));

    scene.run();

    for c in ['H', 'i', '!'] {
        assert!(
            scene.atlas.slot(&key(Font::BUILT_IN, c as u16)).is_some(),
            "{c:?} is resident"
        );
    }
    assert_eq!(scene.atlas.len(), 3);
}

/// **A glyph is keyed by its layout's font and size and its own x.** The
/// text's origin is snapped to whole pixels when drawn, so the subpixel bin
/// comes from the glyph's x within the layout, and the size from the layout.
#[test]
fn a_glyph_is_keyed_by_its_layout_s_font_and_size_and_its_own_x() {
    let mut scene = Scene::new();
    let body = font_handle(BODY, b"body v1");
    scene.world.spawn((
        UiTransform::default(),
        layout(BODY, 15.9, &[(b'a' as u16, 3.27)]),
        body,
    ));

    scene.run();

    let expected = GlyphKey::new(BODY, b'a' as u16, 15.9, 3.27);
    assert_eq!(scene.rasterizer.asked(), vec![expected]);
    assert!(scene.atlas.slot(&expected).is_some());
}

/// **A changed layout makes its new glyphs resident**, without asking again
/// for the ones it already had.
#[test]
fn a_changed_layout_makes_its_new_glyphs_resident() {
    let mut scene = Scene::new();
    let entity = scene.text(Font::BUILT_IN, &chars("a"));
    scene.run();

    scene.relayout(entity, Font::BUILT_IN, &chars("ab"));
    scene.run();

    assert!(scene
        .atlas
        .slot(&key(Font::BUILT_IN, b'b' as u16))
        .is_some());
    assert_eq!(
        scene
            .rasterizer
            .times_asked(&key(Font::BUILT_IN, b'a' as u16)),
        1,
        "'a' was resident already"
    );
}

// --- rasterised once ---

/// **A glyph used twice is rasterised once**, whether twice in one text or
/// once in each of two texts, and uploaded once.
#[test]
fn a_glyph_used_twice_in_one_text_or_two_is_rasterised_once() {
    let mut scene = Scene::new();
    scene.text(Font::BUILT_IN, &[(b'a' as u16, 0.0), (b'a' as u16, 8.0)]);
    scene.text(Font::BUILT_IN, &[(b'a' as u16, 16.0)]);

    scene.run();

    let a = key(Font::BUILT_IN, b'a' as u16);
    assert_eq!(scene.rasterizer.times_asked(&a), 1);
    assert_eq!(scene.atlas.len(), 1);
    let mask = scene.mask_texture().expect("a mask atlas");
    assert_eq!(
        scene.device.glyph_uploads_to(mask).len(),
        1,
        "uploaded once"
    );
}

/// **An unchanged layout rasterises nothing on the next run**, and uploads
/// nothing.
#[test]
fn an_unchanged_layout_rasterises_nothing_on_the_next_run() {
    let mut scene = Scene::new();
    scene.text(Font::BUILT_IN, &chars("Hello"));
    scene.run();
    let asked = scene.rasterizer.asked().len();
    let uploads = scene.device.upload_count();

    scene.run();
    scene.run();

    assert_eq!(scene.rasterizer.asked().len(), asked, "nothing asked again");
    assert_eq!(
        scene.device.upload_count(),
        uploads,
        "nothing uploaded again"
    );
    assert_eq!(scene.atlas.len(), 4, "H, e, l, o");
}

/// **A space gets no slot and is not asked again.** The rasteriser's `None`
/// is remembered as nothing to draw — even when a later layout uses it again.
#[test]
fn a_space_gets_no_slot_and_is_not_asked_again() {
    let mut scene = Scene::new();
    let entity = scene.text(Font::BUILT_IN, &chars("a b"));
    scene.run();

    let space = key(Font::BUILT_IN, SPACE);
    assert_eq!(scene.atlas.slot(&space), None, "a space has no slot");
    assert!(scene
        .atlas
        .slot(&key(Font::BUILT_IN, b'a' as u16))
        .is_some());
    assert_eq!(scene.atlas.len(), 2, "a and b; the space is not placed");

    scene.relayout(entity, Font::BUILT_IN, &chars("c d"));
    scene.text(Font::BUILT_IN, &chars(" "));
    scene.run();

    assert_eq!(
        scene.rasterizer.times_asked(&space),
        1,
        "the space is asked for once"
    );
    assert_eq!(scene.atlas.slot(&space), None);
}

/// **No atlas is made before a glyph needs it**: texts of spaces alone make
/// no texture.
#[test]
fn no_atlas_is_made_before_a_glyph_needs_it() {
    let mut scene = Scene::new();
    scene.text(Font::BUILT_IN, &chars("   "));

    scene.run();

    assert!(
        scene.device.textures().is_empty(),
        "{:?}",
        scene.device.textures()
    );
    assert_eq!(scene.atlas.views(), (None, None));
    assert!(scene.atlas.is_empty());
}

// --- masks and colour ---

/// **Colour glyphs go to the RGBA atlas and masks to the R8 one**: each
/// image is uploaded into the texture of its kind, and its slot says which.
#[test]
fn colour_glyphs_go_to_the_rgba_atlas_and_masks_to_the_r8_one() {
    let mut scene = Scene::new();
    scene.text(Font::BUILT_IN, &[(b'a' as u16, 0.0), (COLOR_GLYPHS, 8.0)]);

    scene.run();

    assert_eq!(scene.device.r8_textures().len(), 1, "one R8 atlas");
    assert_eq!(scene.device.rgba_textures().len(), 1, "one RGBA atlas");
    let mask = scene.mask_texture().expect("a mask view");
    let color = scene.color_texture().expect("a colour view");
    assert_eq!(
        scene.device.texture(mask).map(|t| t.format),
        Some(TextureFormat::R8Unorm),
        "the mask view is the R8 texture's"
    );
    assert!(
        scene
            .device
            .texture(color)
            .is_some_and(|t| is_rgba(t.format)),
        "the colour view is the RGBA texture's"
    );

    let masks = scene.device.glyph_uploads_to(mask);
    assert_eq!(masks.len(), 1);
    assert_eq!((masks[0].width, masks[0].height), (MASK_WIDTH, MASK_HEIGHT));
    assert_eq!(masks[0].bytes, (MASK_WIDTH * MASK_HEIGHT) as usize);
    let colors = scene.device.glyph_uploads_to(color);
    assert_eq!(colors.len(), 1);
    assert_eq!(
        (colors[0].width, colors[0].height),
        (COLOR_WIDTH, COLOR_HEIGHT)
    );
    assert_eq!(colors[0].bytes, (COLOR_WIDTH * COLOR_HEIGHT * 4) as usize);

    let a = scene
        .atlas
        .slot(&key(Font::BUILT_IN, b'a' as u16))
        .expect("'a' is resident");
    let emoji = scene
        .atlas
        .slot(&GlyphKey::new(Font::BUILT_IN, COLOR_GLYPHS, SIZE, 8.0))
        .expect("the emoji is resident");
    assert_eq!(a.kind, GlyphKind::Mask);
    assert_eq!(emoji.kind, GlyphKind::Color);
}

/// **The RGBA atlas is made only when a colour glyph needs it.** Masks alone
/// make the R8 atlas; the first colour glyph makes the RGBA one.
#[test]
fn the_rgba_atlas_is_made_only_when_a_colour_glyph_needs_it() {
    let mut scene = Scene::new();
    scene.text(Font::BUILT_IN, &chars("masks"));
    scene.run();

    assert!(scene.atlas.views().0.is_some(), "the mask atlas is made");
    assert_eq!(scene.atlas.views().1, None, "no colour atlas yet");
    assert!(scene.device.rgba_textures().is_empty());

    scene.text(Font::BUILT_IN, &[(COLOR_GLYPHS, 0.0)]);
    scene.run();

    assert!(scene.atlas.views().1.is_some(), "the colour atlas is made");
    assert_eq!(scene.device.rgba_textures().len(), 1);
}

/// **Each atlas is a square of the size asked.**
#[test]
fn each_atlas_is_a_square_of_the_size_asked() {
    let mut scene = Scene::with_atlas_size(256);
    scene.text(Font::BUILT_IN, &[(b'a' as u16, 0.0), (COLOR_GLYPHS, 8.0)]);

    scene.run();

    let textures = scene.device.textures();
    assert_eq!(textures.len(), 2, "{textures:?}");
    for texture in textures {
        assert_eq!(
            (texture.size.width, texture.size.height),
            (256, 256),
            "{texture:?}"
        );
    }
}

/// **The default atlas is 1024 px square.**
#[test]
fn the_default_atlas_is_1024_pixels_square() {
    let mut scene = Scene::new();
    scene.text(Font::BUILT_IN, &chars("a"));

    scene.run();

    let mask = scene.device.r8_textures();
    assert_eq!(mask.len(), 1);
    assert_eq!((mask[0].size.width, mask[0].size.height), (1024, 1024));
}

// --- subpixel bins ---

/// **Two subpixel bins of one glyph get two slots**: the same glyph at a
/// whole pixel and half a pixel on is rasterised twice and placed twice.
#[test]
fn two_subpixel_bins_of_one_glyph_get_two_slots() {
    let mut scene = Scene::new();
    scene.text(Font::BUILT_IN, &[(b'a' as u16, 0.0), (b'a' as u16, 10.5)]);

    scene.run();

    let whole = GlyphKey::new(Font::BUILT_IN, b'a' as u16, SIZE, 0.0);
    let half = GlyphKey::new(Font::BUILT_IN, b'a' as u16, SIZE, 10.5);
    assert_ne!(whole, half, "two bins");
    let whole_slot = scene.atlas.slot(&whole).expect("the whole-pixel 'a'");
    let half_slot = scene.atlas.slot(&half).expect("the half-pixel 'a'");
    assert_ne!(whole_slot.rect, half_slot.rect, "two places in the atlas");
    assert_eq!(scene.rasterizer.times_asked(&whole), 1);
    assert_eq!(scene.rasterizer.times_asked(&half), 1);
    assert_eq!(scene.atlas.len(), 2);
}

// --- the slot ---

/// **A slot holds the image's size and offset**, and its rect covers the
/// image's pixels in the atlas.
#[test]
fn a_slot_holds_the_image_s_size_and_offset() {
    let mut scene = Scene::new();
    scene.text(Font::BUILT_IN, &[(b'a' as u16, 0.0), (COLOR_GLYPHS, 8.0)]);

    scene.run();

    let side = 1024.0;
    let mask = scene
        .atlas
        .slot(&key(Font::BUILT_IN, b'a' as u16))
        .expect("'a' is resident");
    assert_eq!(
        (mask.width, mask.height, mask.left, mask.top),
        (MASK_WIDTH, MASK_HEIGHT, MASK_LEFT, MASK_TOP)
    );
    let span = mask.rect.max - mask.rect.min;
    assert!(
        (span.x - MASK_WIDTH as f32 / side).abs() < 1e-6
            && (span.y - MASK_HEIGHT as f32 / side).abs() < 1e-6,
        "the rect covers the mask: {:?}",
        mask.rect
    );

    let color = scene
        .atlas
        .slot(&GlyphKey::new(Font::BUILT_IN, COLOR_GLYPHS, SIZE, 8.0))
        .expect("the emoji is resident");
    assert_eq!(
        (color.width, color.height, color.left, color.top),
        (COLOR_WIDTH, COLOR_HEIGHT, COLOR_LEFT, COLOR_TOP)
    );
    let span = color.rect.max - color.rect.min;
    assert!(
        (span.x - COLOR_WIDTH as f32 / side).abs() < 1e-6
            && (span.y - COLOR_HEIGHT as f32 / side).abs() < 1e-6,
        "the rect covers the image: {:?}",
        color.rect
    );
}

// --- a full atlas ---

/// **A full atlas is rebuilt with only the glyphs in use.** A [`TINY`]
/// atlas holds 12 masks: 8 are placed, then one text is laid out again in 8
/// new glyphs. The 5th does not fit, so a new mask atlas is made holding the
/// 10 glyphs the layouts now use — the unchanged text's 2 among them — the
/// old texture and its view are destroyed, the glyphs no text uses any more
/// have no slot, and the mask view is the new one.
#[test]
fn a_full_atlas_is_rebuilt_with_only_the_glyphs_in_use() {
    let mut scene = Scene::with_atlas_size(TINY);
    let changing = scene.text(Font::BUILT_IN, &ids(1..=6));
    scene.text(Font::BUILT_IN, &ids([50, 51]));
    scene.run();
    assert_eq!(scene.atlas.len(), 8);
    let old_view = scene.atlas.views().0.expect("a mask atlas");
    let old_texture = scene.mask_texture().expect("its texture");

    scene.relayout(changing, Font::BUILT_IN, &ids(101..=108));
    scene.run();

    let new_view = scene.atlas.views().0.expect("a mask atlas still");
    assert_ne!(new_view, old_view, "the mask view is the new atlas's");
    let new_texture = scene.mask_texture().expect("its texture");
    assert_ne!(new_texture, old_texture);
    assert!(
        scene.device.destroyed_textures().contains(&old_texture),
        "the old texture is destroyed: {:?}",
        scene.device.destroyed_textures()
    );
    assert!(
        scene.device.destroyed_views().contains(&old_view),
        "the old view is destroyed: {:?}",
        scene.device.destroyed_views()
    );
    assert!(
        !scene.device.destroyed_textures().contains(&new_texture),
        "the new texture lives"
    );

    for id in 1..=6 {
        assert_eq!(
            scene.atlas.slot(&key(Font::BUILT_IN, id)),
            None,
            "glyph {id} is no longer used: its slot went with the old atlas"
        );
    }
    for id in [50, 51].into_iter().chain(101..=108) {
        let slot = scene
            .atlas
            .slot(&key(Font::BUILT_IN, id))
            .unwrap_or_else(|| panic!("glyph {id} is in use, so resident"));
        assert_eq!(slot.kind, GlyphKind::Mask);
    }
    assert_eq!(scene.atlas.len(), 10, "only the glyphs in use");
    assert_eq!(
        scene.device.glyph_uploads_to(new_texture).len(),
        10,
        "the new atlas holds the 10 glyphs in use, each once"
    );
}

/// **Rebuilding the mask atlas leaves the colour atlas alone**: its view, its
/// texture and its glyphs' slots are what they were.
#[test]
fn rebuilding_the_mask_atlas_leaves_the_colour_atlas_alone() {
    let mut scene = Scene::with_atlas_size(TINY);
    let changing = scene.text(Font::BUILT_IN, &ids(1..=8));
    scene.text(Font::BUILT_IN, &[(COLOR_GLYPHS, 0.0)]);
    scene.run();
    let old_mask_view = scene.atlas.views().0.expect("a mask atlas");
    let color_view = scene.atlas.views().1.expect("a colour atlas");
    let color_texture = scene.color_texture().expect("its texture");
    let emoji = key(Font::BUILT_IN, COLOR_GLYPHS);
    let emoji_slot = scene.atlas.slot(&emoji).expect("the emoji is resident");

    scene.relayout(changing, Font::BUILT_IN, &ids(101..=108));
    scene.run();

    assert_ne!(
        scene.atlas.views().0,
        Some(old_mask_view),
        "the mask atlas was rebuilt"
    );
    assert_eq!(
        scene.atlas.views().1,
        Some(color_view),
        "the colour one kept"
    );
    assert!(!scene.device.destroyed_textures().contains(&color_texture));
    assert!(!scene.device.destroyed_views().contains(&color_view));
    assert_eq!(scene.atlas.slot(&emoji), Some(emoji_slot));
}

/// **A full colour atlas is rebuilt without touching the mask atlas.** A
/// [`TINY`] atlas holds 9 colour glyphs: 6 are placed, then their text is
/// laid out again in 6 new ones.
#[test]
fn a_full_colour_atlas_is_rebuilt_without_touching_the_mask_atlas() {
    let mut scene = Scene::with_atlas_size(TINY);
    let changing = scene.text(Font::BUILT_IN, &ids(COLOR_GLYPHS..COLOR_GLYPHS + 6));
    scene.text(Font::BUILT_IN, &chars("a"));
    scene.run();
    let mask_view = scene.atlas.views().0.expect("a mask atlas");
    let mask_texture = scene.mask_texture().expect("its texture");
    let old_color_view = scene.atlas.views().1.expect("a colour atlas");
    let old_color_texture = scene.color_texture().expect("its texture");

    let fresh = COLOR_GLYPHS + 100..COLOR_GLYPHS + 106;
    scene.relayout(changing, Font::BUILT_IN, &ids(fresh.clone()));
    scene.run();

    let new_color_view = scene.atlas.views().1.expect("a colour atlas still");
    assert_ne!(new_color_view, old_color_view);
    assert!(scene
        .device
        .destroyed_textures()
        .contains(&old_color_texture));
    assert!(scene.device.destroyed_views().contains(&old_color_view));
    assert_eq!(
        scene.atlas.views().0,
        Some(mask_view),
        "the mask atlas kept"
    );
    assert!(!scene.device.destroyed_textures().contains(&mask_texture));
    assert!(scene
        .atlas
        .slot(&key(Font::BUILT_IN, b'a' as u16))
        .is_some());

    for id in COLOR_GLYPHS..COLOR_GLYPHS + 6 {
        assert_eq!(scene.atlas.slot(&key(Font::BUILT_IN, id)), None);
    }
    for id in fresh {
        assert!(scene.atlas.slot(&key(Font::BUILT_IN, id)).is_some());
    }
    let new_color_texture = scene.color_texture().expect("its texture");
    assert_eq!(
        scene.device.glyph_uploads_to(new_color_texture).len(),
        6,
        "the new colour atlas holds the 6 glyphs in use"
    );
}

/// **More glyphs in use than an empty atlas holds neither hangs nor
/// panics**: an atlas built again from what is in use is full again, and
/// building it once more would not make room. What fits is placed.
#[test]
fn more_glyphs_in_use_than_an_empty_atlas_holds_neither_hangs_nor_panics() {
    let mut scene = Scene::with_atlas_size(TINY);
    scene.text(Font::BUILT_IN, &ids(1..=20));

    scene.run();
    scene.run();

    assert!(
        scene.atlas.len() <= TINY_MASKS,
        "a {TINY} px atlas holds {TINY_MASKS} masks at most: {}",
        scene.atlas.len()
    );
    assert!(!scene.atlas.is_empty(), "what fits is placed");
}

// --- fonts ---

/// **A reloaded font registers with the rasteriser once, and its glyphs are
/// rasterised again.** The resolver replaces the handle on every text in the
/// font, and `ui_text_layout` lays those texts out again; the glyphs of the
/// old file are not drawn for the new one. A text in another font keeps its
/// glyphs.
#[test]
fn a_reloaded_font_registers_once_and_its_glyphs_are_rasterised_again() {
    let mut scene = Scene::new();
    let v1 = font_handle(BODY, b"body v1");
    let first = scene.text_with_handle(&v1, &chars("a"));
    let second = scene.text_with_handle(&v1, &chars("b"));
    scene.text(Font::BUILT_IN, &chars("a"));
    scene.run();
    assert_eq!(
        scene.rasterizer.registrations(),
        vec![(BODY, b"body v1".to_vec())],
        "the font is registered once, whatever the number of texts"
    );
    let (a, b) = (key(BODY, b'a' as u16), key(BODY, b'b' as u16));
    let built_in_a = key(Font::BUILT_IN, b'a' as u16);
    assert_eq!(scene.rasterizer.times_asked(&a), 1);

    let v2 = font_handle(BODY, b"body v2");
    for (entity, text) in [(first, "a"), (second, "b")] {
        assert!(scene.world.set_component(entity, v2.clone()));
        scene.relayout(entity, BODY, &chars(text));
    }
    scene.run();

    assert_eq!(
        scene.rasterizer.registrations(),
        vec![(BODY, b"body v1".to_vec()), (BODY, b"body v2".to_vec())],
        "the new file is registered once"
    );
    assert_eq!(scene.rasterizer.times_asked(&a), 2, "'a' made again");
    assert_eq!(scene.rasterizer.times_asked(&b), 2, "'b' made again");
    assert!(scene.atlas.slot(&a).is_some());
    assert!(scene.atlas.slot(&b).is_some());
    assert_eq!(
        scene.rasterizer.times_asked(&built_in_a),
        1,
        "the built-in 'a' is another font's: kept"
    );

    scene.run();
    assert_eq!(scene.rasterizer.registrations().len(), 2, "not again");
}

/// **A text spawned in a font already registered registers nothing**: it
/// shares the handle the other texts hold, so its font's file is not new,
/// and the glyphs it shares with them are not made again.
#[test]
fn a_text_spawned_in_a_font_already_registered_registers_nothing() {
    let mut scene = Scene::new();
    let shared = font_handle(BODY, b"body v1");
    scene.text_with_handle(&shared, &chars("a"));
    scene.run();

    scene.text_with_handle(&shared, &chars("ab"));
    scene.run();

    assert_eq!(
        scene.rasterizer.registrations(),
        vec![(BODY, b"body v1".to_vec())]
    );
    assert_eq!(scene.rasterizer.times_asked(&key(BODY, b'a' as u16)), 1);
    assert!(scene.atlas.slot(&key(BODY, b'b' as u16)).is_some());
}

/// **A handle named after the built-in font is never registered**: the
/// built-in face is the engine's, never a handle's to replace.
#[test]
fn a_handle_named_after_the_built_in_font_is_never_registered() {
    let mut scene = Scene::new();
    scene.text(Font::BUILT_IN, &chars("a"));
    scene.world.spawn(font_handle(Font::BUILT_IN, b"impostor"));

    scene.run();

    assert!(
        scene.rasterizer.registrations().is_empty(),
        "{:?}",
        scene.rasterizer.registrations()
    );
    assert!(scene
        .atlas
        .slot(&key(Font::BUILT_IN, b'a' as u16))
        .is_some());
}

// --- a missing entry ---

/// The texts the missing-entry tests run over: one built-in, one in a font
/// with its handle.
fn spawn_texts(scene: &mut Scene) {
    scene.text(Font::BUILT_IN, &chars("Built in"));
    let body = font_handle(BODY, b"body v1");
    scene.text_with_handle(&body, &chars("Body"));
}

/// **Without a device nothing happens**: no font registered, no glyph
/// rasterised, the atlas empty.
#[test]
fn without_a_device_nothing_happens() {
    let mut scene = Scene::build(
        GlyphAtlas::default(),
        Parts {
            device: false,
            ..ALL
        },
    );
    spawn_texts(&mut scene);

    scene.run();
    scene.run();

    assert!(scene.rasterizer.registrations().is_empty());
    assert!(scene.rasterizer.asked().is_empty());
    assert!(scene.atlas.is_empty());
    assert_eq!(scene.atlas.views(), (None, None));
}

/// **Without a rasteriser nothing happens**: no texture made, the atlas
/// empty.
#[test]
fn without_a_rasteriser_nothing_happens() {
    let mut scene = Scene::build(
        GlyphAtlas::default(),
        Parts {
            rasterizer: false,
            ..ALL
        },
    );
    spawn_texts(&mut scene);

    scene.run();
    scene.run();

    assert!(scene.device.textures().is_empty());
    assert!(scene.atlas.is_empty());
    assert_eq!(scene.atlas.views(), (None, None));
}

/// **Without an atlas nothing happens**: no font registered, no glyph
/// rasterised, no texture made.
#[test]
fn without_an_atlas_nothing_happens() {
    let mut scene = Scene::build(
        GlyphAtlas::default(),
        Parts {
            atlas: false,
            ..ALL
        },
    );
    spawn_texts(&mut scene);

    scene.run();
    scene.run();

    assert!(scene.rasterizer.registrations().is_empty());
    assert!(scene.rasterizer.asked().is_empty());
    assert!(scene.device.textures().is_empty());
}

// --- the registration ---

/// **Registered in `PreExtract`, after `ui_text_layout`, observing the
/// layout and the font handle**: the layouts it reads are written by
/// `ui_text_layout` the same tick, and the two observed components are what
/// it reads `Changed` of.
#[test]
fn the_registration_runs_in_pre_extract_after_ui_text_layout_observing_layout_and_font() {
    let registration = inventory::iter::<DataSystemRegistration>
        .into_iter()
        .find(|registration| registration.name == "glyphs_on_gpu")
        .expect("a DataSystem named `glyphs_on_gpu` is registered");

    assert_eq!(registration.phase, TickPhase::PreExtract);
    assert!(
        registration.runs_after.contains(&"ui_text_layout"),
        "after ui_text_layout: {:?}",
        registration.runs_after
    );
    let observed: Vec<ComponentKey> = registration.observes.iter().map(|key| key()).collect();
    assert!(
        observed.contains(&ComponentKey::of::<UiTextLayout>()),
        "observes UiTextLayout"
    );
    assert!(
        observed.contains(&ComponentKey::of::<HandleComponent<Font>>()),
        "observes the font handle"
    );
}

// --- what a full atlas leaves out ---

/// Glyph ids from this one up are, to [`TallRasterizer`], masks taller and
/// wider than a [`TINY`] atlas: no atlas of that size can hold one.
const OVERSIZED: u16 = 2000;

/// The fake rasteriser, but a glyph from [`OVERSIZED`] up is a 40 × 40 mask
/// (not a colour glyph, though its id is past [`COLOR_GLYPHS`]).
#[derive(Default)]
struct TallRasterizer {
    inner: FakeRasterizer,
}

impl GlyphRasterizer for TallRasterizer {
    fn register_font(&self, id: AssetUUID, data: Arc<[u8]>) -> Result<(), FontError> {
        self.inner.register_font(id, data)
    }

    fn rasterize(&self, key: &GlyphKey) -> Option<GlyphImage> {
        if key.glyph >= OVERSIZED {
            locked(&self.inner.asked).push(*key);
            return Some(GlyphImage {
                width: 40,
                height: 40,
                left: 0,
                top: 40,
                kind: GlyphKind::Mask,
                pixels: vec![255; 40 * 40],
            });
        }
        self.inner.rasterize(key)
    }
}

/// **A glyph no atlas can hold does not rebuild the atlas.** A [`TINY`]
/// atlas cannot hold a 40 px glyph, whatever is in it, so that glyph is not
/// a sign the atlas is full. When its text is edited, the mask atlas is kept,
/// the glyphs of a text left alone keep their slots and are not rasterised
/// again.
#[test]
fn a_glyph_no_atlas_can_hold_does_not_rebuild_the_atlas() {
    let mut scene = Scene::with_atlas_size(TINY);
    let tall: Arc<dyn GlyphRasterizer> = Arc::new(TallRasterizer::default());
    scene.runtime.backends.insert(tall);
    let title = scene.text(Font::BUILT_IN, &[(OVERSIZED, 0.0)]);
    scene.text(Font::BUILT_IN, &chars("ab"));
    scene.run();
    for c in ['a', 'b'] {
        assert!(
            scene.atlas.slot(&key(Font::BUILT_IN, c as u16)).is_some(),
            "{c:?} is resident after the first run"
        );
    }
    let mask = scene.mask_texture().expect("a mask atlas");
    let a = key(Font::BUILT_IN, b'a' as u16);
    let asked_for_a = scene.rasterizer.times_asked(&a);

    scene.relayout(
        title,
        Font::BUILT_IN,
        &[(OVERSIZED, 0.0), (b'z' as u16, 48.0)],
    );
    scene.run();

    for c in ['a', 'b', 'z'] {
        assert!(
            scene.atlas.slot(&key(Font::BUILT_IN, c as u16)).is_some(),
            "{c:?} is in use and fits: resident"
        );
    }
    assert!(
        !scene.device.destroyed_textures().contains(&mask),
        "the mask atlas had room: kept"
    );
    assert_eq!(
        scene.rasterizer.times_asked(&a),
        asked_for_a,
        "'a' was resident: not rasterised again"
    );
}

/// **Glyphs a full atlas left out are placed once there is room.** A
/// [`TINY`] atlas holds 12 masks. A text of 12 fills it; a second text of 8
/// is spawned, and the 20 glyphs in use do not all fit, so some go without a
/// slot. The first text is then cleared: the 8 glyphs still in use fit an
/// atlas, and each is resident after the next run.
#[test]
fn glyphs_a_full_atlas_left_out_are_placed_once_there_is_room() {
    let mut scene = Scene::with_atlas_size(TINY);
    let first = scene.text(Font::BUILT_IN, &ids(1..=12));
    scene.run();
    assert_eq!(scene.atlas.len(), TINY_MASKS, "the atlas is full");
    scene.text(Font::BUILT_IN, &ids(20..=27));
    scene.run();

    scene.relayout(first, Font::BUILT_IN, &[]);
    scene.run();

    for id in 20..=27 {
        assert!(
            scene.atlas.slot(&key(Font::BUILT_IN, id)).is_some(),
            "glyph {id} is in use and fits: resident"
        );
    }
}

/// **Rebuilding the mask atlas rasterises no colour glyph nor space again.**
/// Only masks go into the atlas built again; a colour glyph and a space keep
/// what was made of them.
#[test]
fn rebuilding_the_mask_atlas_rasterises_no_colour_glyph_nor_space_again() {
    let mut scene = Scene::with_atlas_size(TINY);
    let changing = scene.text(Font::BUILT_IN, &ids(1..=8));
    scene.text(Font::BUILT_IN, &[(COLOR_GLYPHS, 0.0), (SPACE, 8.0)]);
    scene.run();
    let old_mask = scene.atlas.views().0.expect("a mask atlas");

    scene.relayout(changing, Font::BUILT_IN, &ids(101..=108));
    scene.run();
    assert_ne!(
        scene.atlas.views().0,
        Some(old_mask),
        "the mask atlas was rebuilt"
    );

    let emoji = key(Font::BUILT_IN, COLOR_GLYPHS);
    let space = GlyphKey::new(Font::BUILT_IN, SPACE, SIZE, 8.0);
    assert_eq!(scene.rasterizer.times_asked(&emoji), 1, "the emoji once");
    assert_eq!(scene.rasterizer.times_asked(&space), 1, "the space once");
}

/// **Attaching a layout to a text does not make its font new.** The font's
/// file was registered when its handle arrived; `ui_text_layout` attaching
/// the text's first layout later moves the entity's row, which is not a new
/// file: the rasteriser is not handed the font again, and its glyphs are not
/// made again.
#[test]
fn attaching_a_layout_to_a_text_does_not_make_its_font_new() {
    let mut scene = Scene::new();
    let body = font_handle(BODY, b"body v1");
    scene.text_with_handle(&body, &chars("a"));
    let later = scene.world.spawn((UiTransform::default(), body.clone()));
    scene.run();
    assert_eq!(scene.rasterizer.registrations().len(), 1);

    scene
        .world
        .add_component(later, layout(BODY, SIZE, &chars("a")))
        .expect("the layout attaches");
    scene.run();

    assert_eq!(
        scene.rasterizer.registrations().len(),
        1,
        "the font's file is the one registered"
    );
    assert_eq!(scene.rasterizer.times_asked(&key(BODY, b'a' as u16)), 1);
}

/// **A rebuild places every glyph in use that fits, past one that does
/// not.** A text whose one glyph no [`TINY`] atlas can hold comes first; a
/// second text is laid out again in 8 new masks and fills the atlas, which
/// is built again: the 8 masks in use fit a fresh atlas, so each is
/// resident, whatever stands before them.
#[test]
fn a_rebuild_places_every_glyph_in_use_that_fits_past_one_that_does_not() {
    let mut scene = Scene::with_atlas_size(TINY);
    let tall: Arc<dyn GlyphRasterizer> = Arc::new(TallRasterizer::default());
    scene.runtime.backends.insert(tall);
    scene.text(Font::BUILT_IN, &[(OVERSIZED, 0.0)]);
    let body = scene.text(Font::BUILT_IN, &ids(1..=8));
    scene.run();

    scene.relayout(body, Font::BUILT_IN, &ids(101..=108));
    scene.run();

    for id in 101..=108 {
        assert!(
            scene.atlas.slot(&key(Font::BUILT_IN, id)).is_some(),
            "glyph {id} is in use and fits: resident"
        );
    }
}

/// **Glyphs in use that only grew rebuild no atlas.** Once a full atlas has
/// left glyphs out, a text appearing in glyphs that take no room in the mask
/// atlas — a space, an emoji — frees nothing there: the mask atlas is kept,
/// and its glyphs are not rasterised again.
#[test]
fn glyphs_in_use_that_only_grew_rebuild_no_atlas() {
    let mut scene = Scene::with_atlas_size(TINY);
    scene.text(Font::BUILT_IN, &ids(1..=12));
    scene.run();
    scene.text(Font::BUILT_IN, &ids(20..=27));
    scene.run();
    let mask = scene.mask_texture().expect("a mask atlas");
    let one = key(Font::BUILT_IN, 1);
    let asked_for_one = scene.rasterizer.times_asked(&one);

    scene.text(Font::BUILT_IN, &[(SPACE, 0.0), (COLOR_GLYPHS, 8.0)]);
    scene.run();

    assert!(
        !scene.device.destroyed_textures().contains(&mask),
        "nothing left the glyphs in use: the mask atlas is kept"
    );
    assert_eq!(scene.rasterizer.times_asked(&one), asked_for_one);
}

/// **A glyph is rasterised once in a run that rebuilds the other atlas.**
/// A text laid out again in new masks and an emoji fills the mask atlas,
/// which is built again; the emoji, new too, is rasterised once all the
/// same.
#[test]
fn a_glyph_is_rasterised_once_in_a_run_that_rebuilds_the_other_atlas() {
    let mut scene = Scene::with_atlas_size(TINY);
    let changing = scene.text(Font::BUILT_IN, &ids(1..=8));
    scene.run();

    let mut glyphs = ids(101..=108);
    glyphs.push((COLOR_GLYPHS, 72.0));
    scene.relayout(changing, Font::BUILT_IN, &glyphs);
    scene.run();

    let emoji = key(Font::BUILT_IN, COLOR_GLYPHS);
    assert!(scene.atlas.slot(&emoji).is_some(), "the emoji is resident");
    assert_eq!(scene.rasterizer.times_asked(&emoji), 1, "the emoji once");
}

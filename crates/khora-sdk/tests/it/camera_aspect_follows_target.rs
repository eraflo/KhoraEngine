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

//! A camera's aspect is the shape of the image it fills.
//!
//! The engine publishes the colour target's size in the frame's
//! `FrameContext` as `ColorTargetSize`; the render and shadow flows project
//! the active camera at that aspect, and a resize — which moves no ECS epoch —
//! changes their cache keys so no stale view is served.

use std::sync::Arc;

use khora_sdk::khora_core::lane::ColorTargetSize;
use khora_sdk::khora_core::math::{Extent2D, Mat4, Vec3};
use khora_sdk::khora_core::renderer::api::frame::FrameContext;
use khora_sdk::khora_core::Runtime;
use khora_sdk::khora_data::ecs::{Camera, GlobalTransform, World};
use khora_sdk::khora_data::flow::{Flow, RenderFlow, Selection, ShadowFlow};
use khora_sdk::khora_data::render::{target_aspect, FALLBACK_ASPECT};

/// A frame's runtime as the engine builds it: an `Arc<FrameContext>` among
/// the resources, the tokio runtime it was made with kept alive beside it.
struct Frame {
    _tokio: tokio::runtime::Runtime,
    context: Arc<FrameContext>,
    runtime: Runtime,
}

impl Frame {
    /// A frame whose context holds no colour target size yet.
    fn without_a_size() -> Self {
        let tokio = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("a tokio runtime");
        let context = Arc::new(FrameContext::new(tokio.handle().clone()));
        let mut runtime = Runtime::new();
        runtime.resources.insert(Arc::clone(&context));
        Self {
            _tokio: tokio,
            context,
            runtime,
        }
    }

    /// A frame rendering into a `width`×`height` colour target.
    fn rendering_into(width: u32, height: u32) -> Self {
        let frame = Self::without_a_size();
        frame.resize(width, height);
        frame
    }

    /// The colour target is now `width`×`height`, as the engine publishes it
    /// at the start of a frame.
    fn resize(&self, width: u32, height: u32) {
        self.context
            .insert(ColorTargetSize(Extent2D { width, height }));
    }
}

/// A world holding one active perspective camera at `position`, and that
/// camera.
fn world_with_camera(position: Vec3) -> (World, Camera) {
    let camera = Camera::new_perspective(std::f32::consts::FRAC_PI_4, 0.1, 100.0);
    let mut world = World::new();
    world.spawn((GlobalTransform::at_position(position), camera));
    (world, camera)
}

fn assert_mat_close(actual: Mat4, expected: Mat4, what: &str) {
    for (a, e) in actual.cols.iter().zip(expected.cols.iter()) {
        for (x, y) in [(a.x, e.x), (a.y, e.y), (a.z, e.z), (a.w, e.w)] {
            assert!(
                (x - y).abs() < 1e-5,
                "{what}: {actual:?} where {expected:?} was expected"
            );
        }
    }
}

/// The render view `RenderFlow` projects for `world` in `frame`.
fn render_view_proj(world: &World, frame: &Frame) -> Mat4 {
    let view = RenderFlow.project(world, &Selection::new(), &frame.runtime);
    assert_eq!(view.views.len(), 1, "one active camera, one view");
    view.views[0].view_proj
}

/// The aspect a frame's runtime gives is its colour target's: 1024×768 is
/// 4:3.
#[test]
fn the_target_aspect_is_the_color_target_size_in_the_frame_context() {
    let frame = Frame::rendering_into(1024, 768);
    assert_eq!(target_aspect(&frame.runtime), 4.0 / 3.0);
    frame.resize(800, 800);
    assert_eq!(target_aspect(&frame.runtime), 1.0);
}

/// A frame context that holds no colour target size (no target acquired
/// this frame) gives the fallback aspect.
#[test]
fn a_frame_context_without_a_size_gives_the_fallback_aspect() {
    let frame = Frame::without_a_size();
    assert_eq!(target_aspect(&frame.runtime), FALLBACK_ASPECT);
}

/// Rendering into a 1024×768 target, the render view is the active camera's
/// projection at 4/3 times its view — a sphere drawn round.
#[test]
fn the_render_view_is_projected_at_the_target_aspect() {
    let position = Vec3::new(1.0, 2.0, 5.0);
    let (world, camera) = world_with_camera(position);
    let frame = Frame::rendering_into(1024, 768);

    let expected = camera.projection_matrix(4.0 / 3.0).expect("a valid camera")
        * Mat4::from_translation(-position);
    assert_mat_close(render_view_proj(&world, &frame), expected, "view_proj");
}

/// After the target becomes 800×800, the same camera projects at aspect 1.
#[test]
fn the_render_view_follows_a_resize() {
    let position = Vec3::new(0.0, 0.0, 3.0);
    let (world, camera) = world_with_camera(position);
    let frame = Frame::rendering_into(1024, 768);
    let before = render_view_proj(&world, &frame);

    frame.resize(800, 800);
    let after = render_view_proj(&world, &frame);

    let expected =
        camera.projection_matrix(1.0).expect("a valid camera") * Mat4::from_translation(-position);
    assert_mat_close(after, expected, "view_proj after the resize");
    assert_ne!(before, after, "the resize changed nothing");
}

/// A resize moves no ECS epoch, so the render and shadow flows fold the
/// target's aspect into their cache keys: a new size is a new key (no stale
/// view served), the same size the same key (no needless re-projection).
#[test]
fn a_resize_changes_the_render_and_shadow_cache_keys() {
    let (world, _) = world_with_camera(Vec3::new(0.0, 1.0, 4.0));
    let frame = Frame::rendering_into(1024, 768);

    let render = RenderFlow.cache_key(&world, &frame.runtime);
    let shadow = ShadowFlow.cache_key(&world, &frame.runtime);
    assert!(render.is_some() && shadow.is_some(), "both flows cache");

    frame.resize(1024, 768);
    assert_eq!(
        RenderFlow.cache_key(&world, &frame.runtime),
        render,
        "the same size keeps the render key"
    );
    assert_eq!(
        ShadowFlow.cache_key(&world, &frame.runtime),
        shadow,
        "the same size keeps the shadow key"
    );

    frame.resize(800, 800);
    assert_ne!(
        RenderFlow.cache_key(&world, &frame.runtime),
        render,
        "a resize must change the render key"
    );
    assert_ne!(
        ShadowFlow.cache_key(&world, &frame.runtime),
        shadow,
        "a resize must change the shadow key"
    );
}

/// `primary_view` — the view the shadow cascades are fitted around, and the
/// render view when no scene camera is active — projects the active camera
/// at the colour target's aspect, as the render view does.
#[test]
fn the_primary_view_is_projected_at_the_target_aspect() {
    use khora_sdk::khora_data::render::primary_view;

    let position = Vec3::new(-2.0, 1.0, 6.0);
    let (world, camera) = world_with_camera(position);
    let frame = Frame::rendering_into(1024, 768);

    let view = primary_view(&world, &frame.runtime).expect("an active camera");
    let expected = camera.projection_matrix(4.0 / 3.0).expect("a valid camera")
        * Mat4::from_translation(-position);
    assert_mat_close(view.view_proj, expected, "primary view_proj");
    assert_mat_close(
        view.view_proj,
        render_view_proj(&world, &frame),
        "primary view and render view",
    );
}

/// On a 21:9 target, the directional shadow cascade covers the whole slice
/// of the frustum the camera renders — its wide edges included — so a
/// caster at the side of the image still casts a shadow.
#[test]
fn the_shadow_cascade_covers_the_frustum_at_the_target_aspect() {
    use khora_sdk::khora_core::math::Vec4;
    use khora_sdk::khora_core::renderer::light::{DirectionalLight, LightType};
    use khora_sdk::khora_data::ecs::Light;
    use khora_sdk::khora_data::flow::ShadowMatrices;

    let (mut world, _) = world_with_camera(Vec3::new(0.0, 2.0, 0.0));
    world.spawn((
        GlobalTransform::at_position(Vec3::ZERO),
        Light::new(LightType::Directional(DirectionalLight {
            direction: Vec3::new(0.0, -1.0, 0.0),
            shadow_enabled: true,
            ..DirectionalLight::default()
        })),
    ));
    let frame = Frame::rendering_into(2560, 1080);

    let render = render_view_proj(&world, &frame);
    let shadows = ShadowFlow.project(&world, &Selection::new(), &frame.runtime);
    let Some(ShadowMatrices::Single(shadow)) = shadows.matrices.get(&0) else {
        panic!("the directional light casts one shadow matrix: {shadows:?}");
    };

    // The render frustum's corners at the near plane and 55 units along each
    // edge — inside the 60 units the cascade reaches — must land inside the
    // shadow map. A cascade fitted to a square frustum reaches as far along
    // the view axis but not as wide: the side corners fall outside.
    let inverse = render.inverse().expect("an invertible view");
    for x in [-1.0_f32, 1.0] {
        for y in [-1.0_f32, 1.0] {
            let near_h = inverse * Vec4::new(x, y, 0.0, 1.0);
            let far_h = inverse * Vec4::new(x, y, 1.0, 1.0);
            let near = near_h.truncate() / near_h.w;
            let far = far_h.truncate() / far_h.w;
            let edge = far - near;
            let reach = near + edge * (55.0 / edge.length());
            for corner in [near, reach] {
                let clip = *shadow * Vec4::new(corner.x, corner.y, corner.z, 1.0);
                let (sx, sy) = (clip.x / clip.w, clip.y / clip.w);
                assert!(
                    sx.abs() <= 1.01 && sy.abs() <= 1.01,
                    "frustum corner {corner:?} (ndc {x}, {y}) falls outside the shadow map at ({sx}, {sy})"
                );
            }
        }
    }
}

/// A renderer whose colour target is `size`: all `begin_render_frame` asks of
/// it is the frame's targets.
#[derive(Debug)]
struct TargetOfSize(Extent2D);

impl khora_sdk::RenderSystem for TargetOfSize {
    fn init(
        &mut self,
        _window: &dyn khora_sdk::khora_core::platform::window::KhoraWindow,
    ) -> Result<
        Vec<Arc<dyn khora_sdk::khora_core::telemetry::ResourceMonitor>>,
        khora_sdk::khora_core::renderer::error::RenderError,
    > {
        Ok(Vec::new())
    }
    fn resize(&mut self, width: u32, height: u32) {
        self.0 = Extent2D { width, height };
    }
    fn prepare_frame(
        &mut self,
        _view_info: &khora_sdk::khora_core::renderer::api::resource::ViewInfo,
    ) {
    }
    fn render(
        &mut self,
        _renderables: &[khora_sdk::khora_core::renderer::api::gpu_scene::RenderObject],
        _view_info: &khora_sdk::khora_core::renderer::api::resource::ViewInfo,
        _settings: &khora_sdk::khora_core::renderer::api::device::RenderSettings,
    ) -> Result<
        khora_sdk::khora_core::renderer::api::device::RenderStats,
        khora_sdk::khora_core::renderer::error::RenderError,
    > {
        unimplemented!("not drawn")
    }
    fn get_last_frame_stats(&self) -> &khora_sdk::khora_core::renderer::api::device::RenderStats {
        unimplemented!("not drawn")
    }
    fn supports_feature(&self, _feature_name: &str) -> bool {
        false
    }
    fn get_adapter_info(
        &self,
    ) -> Option<khora_sdk::khora_core::renderer::api::device::GraphicsAdapterInfo> {
        None
    }
    fn graphics_device(&self) -> Arc<dyn khora_sdk::khora_core::renderer::GraphicsDevice> {
        unimplemented!("no device")
    }
    fn begin_frame(
        &mut self,
    ) -> Result<
        khora_sdk::khora_core::renderer::traits::FrameTargets,
        khora_sdk::khora_core::renderer::error::RenderError,
    > {
        use khora_sdk::khora_core::renderer::api::resource::TextureViewId;
        Ok(khora_sdk::khora_core::renderer::traits::FrameTargets {
            color: TextureViewId(1),
            depth: None,
            size: self.0,
        })
    }
    fn end_frame(
        &mut self,
    ) -> Result<
        khora_sdk::khora_core::renderer::api::device::RenderStats,
        khora_sdk::khora_core::renderer::error::RenderError,
    > {
        unimplemented!("not presented")
    }
    fn shutdown(&mut self) {}
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// An application with nothing of its own: the engine alone runs the frame.
struct Bare;

impl khora_sdk::AgentProvider for Bare {
    fn register_agents(&self, _dcc: &khora_sdk::DccService, _runtime: &mut Runtime) {}
}
impl khora_sdk::PhaseProvider for Bare {}
impl khora_sdk::EngineApp for Bare {
    fn window_config() -> khora_sdk::WindowConfig {
        khora_sdk::WindowConfig::default()
    }
    fn new() -> Self {
        Bare
    }
    fn setup(&mut self, _world: &mut khora_sdk::GameWorld, _runtime: &Runtime) {}
    fn update(&mut self, _world: &mut khora_sdk::GameWorld, _inputs: &[khora_sdk::InputEvent]) {}
}

/// Acquiring the frame's targets publishes the colour target's size in the
/// frame context — what every camera's projection reads its aspect from —
/// and a resize reaches it on the next frame.
#[test]
fn beginning_a_frame_publishes_the_color_target_size() {
    use std::sync::Mutex;

    let frame = Frame::without_a_size();
    let mut runtime = Runtime::new();
    runtime.resources.insert(Arc::clone(&frame.context));
    let renderer: Arc<Mutex<Box<dyn khora_sdk::RenderSystem>>> =
        Arc::new(Mutex::new(Box::new(TargetOfSize(Extent2D {
            width: 1024,
            height: 768,
        }))));
    runtime.backends.insert(Arc::clone(&renderer));

    let mut engine = khora_sdk::EngineCore::<Bare>::new();
    engine.bootstrap(Bare, runtime);
    let runtime = Arc::clone(engine.runtime());

    assert!(
        engine.begin_render_frame(&runtime),
        "the targets are acquired"
    );
    assert_eq!(
        frame.context.get::<ColorTargetSize>().map(|size| *size),
        Some(ColorTargetSize(Extent2D {
            width: 1024,
            height: 768
        })),
        "the frame context holds the colour target's size"
    );
    assert_eq!(target_aspect(&runtime), 4.0 / 3.0);

    renderer.lock().expect("the renderer").resize(800, 800);
    assert!(
        engine.begin_render_frame(&runtime),
        "the targets are acquired"
    );
    assert_eq!(target_aspect(&runtime), 1.0, "the resize reached the frame");
}

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

//! The two hot-reload pumps read the engine's one `AssetWatch` — an
//! `Arc<AssetWatch>` in the runtime's resources — and take the root of each
//! change from the change itself: a runtime may watch the engine's shader
//! tree and a project's assets at once, and each pump hears the roots it
//! cares about whatever else is watched.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use khora_core::lane::OutputDeck;
use khora_core::renderer::api::command::{
    BindGroupDescriptor, BindGroupId, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
    BindGroupLayoutId, CommandBufferId,
};
use khora_core::renderer::api::device::GraphicsAdapterInfo;
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
use khora_data::ecs::{DataSystemRegistration, World};
use khora_io::asset::AssetWatch;
use khora_io::script::declarations::declaration_channel;
use khora_io::script::hot_reload::{reload_channel, PendingReloads};

/// How long a save may take to reach a pump.
const MAX_WAIT: Duration = Duration::from_secs(10);

/// The registered data system called `name`.
fn data_system(name: &str) -> &'static DataSystemRegistration {
    inventory::iter::<DataSystemRegistration>
        .into_iter()
        .find(|system| system.name == name)
        .unwrap_or_else(|| panic!("`{name}` is registered"))
}

/// Runs the data system `name` over `runtime` until `done` holds, or the wait
/// ran out. The watch reports from the OS's own thread.
fn run_until(name: &str, runtime: &Runtime, mut done: impl FnMut() -> bool) {
    let system = data_system(name);
    let mut world = World::new();
    let deadline = Instant::now() + MAX_WAIT;
    while !done() && Instant::now() < deadline {
        (system.run)(
            &mut world,
            runtime,
            &mut OutputDeck::new(),
            khora_data::ecs::SystemContext {
                since: khora_data::ecs::Tick(0),
            },
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// A runtime holding the engine's watch with every root of `roots` armed, and
/// the queues the script pump fills.
fn watching(roots: &[&Path]) -> (Runtime, Arc<AssetWatch>) {
    let watch = Arc::new(AssetWatch::default());
    for root in roots {
        watch.watch(root).expect("watches the root");
    }
    let mut runtime = Runtime::default();
    runtime.resources.insert(Arc::clone(&watch));
    runtime.resources.insert(reload_channel());
    runtime.resources.insert(declaration_channel());
    std::thread::sleep(Duration::from_millis(150));
    (runtime, watch)
}

/// The modules the script pump sent so far, leaving them queued.
fn sent(runtime: &Runtime) -> Vec<String> {
    let pending = runtime
        .resources
        .get::<PendingReloads>()
        .expect("a reload channel");
    let drained = pending.drain();
    let modules = drained.iter().map(|reload| reload.module.clone()).collect();
    for reload in drained {
        pending.send(reload);
    }
    modules
}

/// An assets tree holding `scripts/game/guard.erg`; returns the module's file.
fn assets_with_guard(assets: &Path) -> std::path::PathBuf {
    let game = assets.join("scripts/game");
    std::fs::create_dir_all(&game).expect("makes the script root");
    let guard = game.join("guard.erg");
    std::fs::write(&guard, "behavior Guard { int ticks = 0; }").expect("writes");
    guard
}

// ─── The script pump ────────────────────────────────────────────────────────

/// **A script saved under a watched root is recompiled** — the pump reads
/// the engine's `AssetWatch`, not a watcher of its own.
#[test]
fn the_script_pump_recompiles_a_save_under_a_watched_root() {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    let guard = assets_with_guard(assets.path());
    let (runtime, _watch) = watching(&[assets.path()]);

    std::fs::write(&guard, "behavior Guard { int ticks = 1; }").expect("edits");
    run_until("script_hot_reload", &runtime, || !sent(&runtime).is_empty());

    assert!(
        sent(&runtime)
            .iter()
            .any(|module| module == "game/guard.erg"),
        "the saved module was recompiled: {:?}",
        sent(&runtime)
    );
}

/// **The script root is found under the root the change came from.** The
/// engine's shader tree is watched first, beside the project: a pump that
/// looked for the scripts under the first root it knew would find none and
/// send nothing.
#[test]
fn the_script_pump_compiles_against_the_root_the_change_came_from() {
    let shaders = tempfile::TempDir::new().expect("a temp dir");
    std::fs::create_dir_all(shaders.path().join("lib")).expect("makes a shader tree");
    let assets = tempfile::TempDir::new().expect("a temp dir");
    let guard = assets_with_guard(assets.path());
    let (runtime, _watch) = watching(&[shaders.path(), assets.path()]);

    std::fs::write(&guard, "behavior Guard { int ticks = 2; }").expect("edits");
    run_until("script_hot_reload", &runtime, || !sent(&runtime).is_empty());

    assert_eq!(
        sent(&runtime),
        vec!["game/guard.erg".to_owned()],
        "the module compiled from the project's own script root"
    );
}

// ─── The shader pump ────────────────────────────────────────────────────────

/// A pipeline system that records the overlays it is handed and builds
/// nothing.
#[derive(Default)]
struct RecordingPipelines {
    overlays: Mutex<Vec<(String, String)>>,
}

impl RecordingPipelines {
    fn overlays(&self) -> Vec<(String, String)> {
        self.overlays.lock().expect("the overlay lock").clone()
    }
}

impl PipelineSystem for RecordingPipelines {
    fn layout(
        &self,
        _device: &dyn GraphicsDevice,
        _key: LayoutKey,
        _variant: &ShaderVariantKey,
    ) -> Result<BindGroupLayoutId, RenderError> {
        unreachable!("the shader pump builds no layout")
    }
    fn inline_layout(
        &self,
        _device: &dyn GraphicsDevice,
        _label: &'static str,
        _entries: &[BindGroupLayoutEntry],
    ) -> Result<BindGroupLayoutId, RenderError> {
        unreachable!("the shader pump builds no layout")
    }
    fn pipeline(
        &self,
        _device: &dyn GraphicsDevice,
        _spec: &PipelineSpec,
    ) -> Result<RenderPipelineId, RenderError> {
        unreachable!("the shader pump builds no pipeline")
    }
    fn compute_pipeline(
        &self,
        _device: &dyn GraphicsDevice,
        _spec: &ComputePipelineSpec,
    ) -> Result<ComputePipelineId, RenderError> {
        unreachable!("the shader pump builds no pipeline")
    }
    fn set_overlay_source(&self, logical_path: &str, source: String) {
        self.overlays
            .lock()
            .expect("the overlay lock")
            .push((logical_path.to_owned(), source));
    }
    fn recompose_dirty(&self, _device: &dyn GraphicsDevice) -> Result<(), RenderError> {
        Ok(())
    }
}

/// A device the shader pump only hands on to `recompose_dirty`: it is never
/// asked to do anything.
#[derive(Debug)]
struct UnusedDevice;

impl GraphicsDevice for UnusedDevice {
    fn create_shader_module(
        &self,
        _d: &ShaderModuleDescriptor,
    ) -> Result<ShaderModuleId, ResourceError> {
        unreachable!()
    }
    fn destroy_shader_module(&self, _id: ShaderModuleId) -> Result<(), ResourceError> {
        unreachable!()
    }
    fn create_render_pipeline(
        &self,
        _d: &RenderPipelineDescriptor,
    ) -> Result<RenderPipelineId, ResourceError> {
        unreachable!()
    }
    fn create_pipeline_layout(
        &self,
        _d: &PipelineLayoutDescriptor,
    ) -> Result<PipelineLayoutId, ResourceError> {
        unreachable!()
    }
    fn destroy_render_pipeline(&self, _id: RenderPipelineId) -> Result<(), ResourceError> {
        unreachable!()
    }
    fn create_compute_pipeline(
        &self,
        _d: &ComputePipelineDescriptor,
    ) -> Result<ComputePipelineId, ResourceError> {
        unreachable!()
    }
    fn destroy_compute_pipeline(&self, _id: ComputePipelineId) -> Result<(), ResourceError> {
        unreachable!()
    }
    fn create_bind_group_layout(
        &self,
        _d: &BindGroupLayoutDescriptor,
    ) -> Result<BindGroupLayoutId, ResourceError> {
        unreachable!()
    }
    fn create_bind_group(&self, _d: &BindGroupDescriptor) -> Result<BindGroupId, ResourceError> {
        unreachable!()
    }
    fn destroy_bind_group_layout(&self, _id: BindGroupLayoutId) -> Result<(), ResourceError> {
        unreachable!()
    }
    fn destroy_bind_group(&self, _id: BindGroupId) -> Result<(), ResourceError> {
        unreachable!()
    }
    fn create_buffer(&self, _d: &BufferDescriptor) -> Result<BufferId, ResourceError> {
        unreachable!()
    }
    fn create_buffer_with_data(
        &self,
        _d: &BufferDescriptor,
        _data: &[u8],
    ) -> Result<BufferId, ResourceError> {
        unreachable!()
    }
    fn destroy_buffer(&self, _id: BufferId) -> Result<(), ResourceError> {
        unreachable!()
    }
    fn write_buffer(&self, _id: BufferId, _offset: u64, _data: &[u8]) -> Result<(), ResourceError> {
        unreachable!()
    }
    fn write_buffer_async<'a>(
        &'a self,
        _id: BufferId,
        _offset: u64,
        _data: &'a [u8],
    ) -> Box<dyn std::future::Future<Output = Result<(), ResourceError>> + Send + 'static> {
        unreachable!()
    }
    fn create_texture(&self, _d: &TextureDescriptor) -> Result<TextureId, ResourceError> {
        unreachable!()
    }
    fn destroy_texture(&self, _id: TextureId) -> Result<(), ResourceError> {
        unreachable!()
    }
    fn write_texture(
        &self,
        _id: TextureId,
        _data: &[u8],
        _bytes_per_row: Option<u32>,
        _offset: khora_core::math::dimension::Origin3D,
        _size: khora_core::math::dimension::Extent3D,
    ) -> Result<(), ResourceError> {
        unreachable!()
    }
    fn create_texture_view(
        &self,
        _id: TextureId,
        _d: &TextureViewDescriptor,
    ) -> Result<TextureViewId, ResourceError> {
        unreachable!()
    }
    fn destroy_texture_view(&self, _id: TextureViewId) -> Result<(), ResourceError> {
        unreachable!()
    }
    fn create_sampler(&self, _d: &SamplerDescriptor) -> Result<SamplerId, ResourceError> {
        unreachable!()
    }
    fn destroy_sampler(&self, _id: SamplerId) -> Result<(), ResourceError> {
        unreachable!()
    }
    fn create_command_encoder(&self, _label: Option<&str>) -> Box<dyn CommandEncoder> {
        unreachable!()
    }
    fn submit_command_buffer(&self, _command_buffer: CommandBufferId) {
        unreachable!()
    }
    fn get_surface_format(&self) -> Option<TextureFormat> {
        unreachable!()
    }
    fn get_surface_size(&self) -> (u32, u32) {
        unreachable!()
    }
    fn get_adapter_info(&self) -> GraphicsAdapterInfo {
        unreachable!()
    }
    fn supports_feature(&self, _feature_name: &str) -> bool {
        unreachable!()
    }
}

/// **A shader saved under a watched root is overlaid** with its new source,
/// under its logical name — the shader pump reads the engine's `AssetWatch`
/// too, the shader tree beside a project's assets as the sandbox watches
/// them.
#[test]
fn the_shader_pump_overlays_a_shader_saved_under_a_watched_root() {
    let shaders = tempfile::TempDir::new().expect("a temp dir");
    let camera = shaders.path().join("lib/std/camera.wgsl");
    std::fs::create_dir_all(camera.parent().expect("a parent")).expect("makes the shader tree");
    std::fs::write(&camera, "// before").expect("writes");
    let assets = tempfile::TempDir::new().expect("a temp dir");
    let (mut runtime, _watch) = watching(&[assets.path(), shaders.path()]);

    let pipelines = Arc::new(RecordingPipelines::default());
    let as_system: Arc<dyn PipelineSystem> = pipelines.clone();
    runtime.resources.insert(as_system);
    let device: Arc<dyn GraphicsDevice> = Arc::new(UnusedDevice);
    runtime.backends.insert(device);

    std::fs::write(&camera, "// after").expect("edits");
    run_until("shader_hot_reload", &runtime, || {
        pipelines
            .overlays()
            .iter()
            .any(|(_, source)| source == "// after")
    });

    assert!(
        pipelines
            .overlays()
            .contains(&("khora::std::camera".to_owned(), "// after".to_owned())),
        "the saved shader was overlaid under its logical name: {:?}",
        pipelines.overlays()
    );
}

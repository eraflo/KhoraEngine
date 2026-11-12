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

//! **What a restricted project does not run is listed in the problems.** A
//! shader overlay a restricted project saves is ignored — the shader pump
//! hands it to no pipeline system — and the problems say so under
//! `ProblemSource::Restricted`: a warning, `Shader overlay ignored`, naming
//! the file and what to do.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use khora_core::lane::OutputDeck;
use khora_core::problems::{ProblemSeverity, ProblemSource, Problems};
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
use khora_io::trust::{ProjectTrust, ProjectTrustState};

/// How long a save may take to reach the pump.
const MAX_WAIT: Duration = Duration::from_secs(10);

/// Runs the shader pump over `runtime` until `done` holds, or the wait ran
/// out. The watch reports from the OS's own thread.
fn run_until(runtime: &Runtime, mut done: impl FnMut() -> bool) {
    let system = inventory::iter::<DataSystemRegistration>
        .into_iter()
        .find(|system| system.name == "shader_hot_reload")
        .expect("`shader_hot_reload` is registered");
    let mut world = World::new();
    let deadline = Instant::now() + MAX_WAIT;
    while !done() && Instant::now() < deadline {
        (system.run)(
            &mut world,
            runtime,
            &mut OutputDeck::new(),
            khora_data::ecs::SystemContext {
                since: khora_data::ecs::Tick(0),
                resumed: false,
                scope: None,
            },
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// A project's assets, holding `shaders/lighting.wgsl`, watched by a runtime
/// whose project is open at `trust`, with the engine's problems, a recording
/// pipeline system and a device. Returns the runtime, the problems, the
/// pipelines and the shader file; the folder goes with the `TempDir`.
fn project_at(
    trust: ProjectTrust,
) -> (
    Runtime,
    Arc<Problems>,
    Arc<RecordingPipelines>,
    std::path::PathBuf,
    tempfile::TempDir,
) {
    let assets = tempfile::TempDir::new().expect("a temp dir");
    let lighting = assets.path().join("shaders").join("lighting.wgsl");
    std::fs::create_dir_all(lighting.parent().expect("a parent")).expect("the shader folder");
    std::fs::write(&lighting, "// before").expect("writes");
    let watch = Arc::new(AssetWatch::default());
    watch.watch(assets.path()).expect("watches the assets");
    let mut runtime = Runtime::default();
    runtime.resources.insert(Arc::clone(&watch));
    let problems = Arc::new(Problems::default());
    runtime.resources.insert(Arc::clone(&problems));
    runtime
        .resources
        .insert(Arc::new(ProjectTrustState::new(trust)));
    let pipelines = Arc::new(RecordingPipelines::default());
    let as_system: Arc<dyn PipelineSystem> = pipelines.clone();
    runtime.resources.insert(as_system);
    let device: Arc<dyn GraphicsDevice> = Arc::new(UnusedDevice);
    runtime.backends.insert(device);
    std::thread::sleep(Duration::from_millis(150));
    (runtime, problems, pipelines, lighting, assets)
}

/// Whether `module` names the file at `file`, with either separator.
fn names(module: Option<&str>, file: &Path) -> bool {
    let Some(module) = module else {
        return false;
    };
    let tail = format!(
        "shaders/{}",
        file.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
    );
    module.replace('\\', "/").ends_with(&tail)
}

/// **A shader overlay a restricted project saves is listed as ignored**:
/// under `ProblemSource::Restricted`, one warning `Shader overlay ignored`
/// whose module is the file, with a note on what to do; the overlay reaches
/// no pipeline system.
#[test]
fn a_restricted_projects_overlay_is_listed_as_ignored() {
    let (runtime, problems, pipelines, lighting, _assets) = project_at(ProjectTrust::Restricted);

    let watch = runtime
        .resources
        .get::<Arc<AssetWatch>>()
        .cloned()
        .expect("the runtime holds the watch");
    std::fs::write(&lighting, "// restricted").expect("edits");
    let mut heard = false;
    run_until(&runtime, || {
        heard |= watch
            .poll_for("restricted_problems_witness")
            .iter()
            .any(|event| event.rel_path.ends_with("lighting.wgsl"));
        heard && !problems.of_source(&ProblemSource::Restricted).is_empty()
    });
    assert!(heard, "the premise: the save reached the watch");

    let listed = problems.of_source(&ProblemSource::Restricted);
    assert_eq!(
        listed.len(),
        1,
        "one problem for the one overlay: {listed:?}"
    );
    let problem = &listed[0];
    assert_eq!(problem.severity, ProblemSeverity::Warning, "a warning");
    assert!(
        problem.message.contains("Shader overlay ignored"),
        "it says the overlay is ignored: {:?}",
        problem.message
    );
    assert!(
        names(problem.module.as_deref(), &lighting),
        "it names the file: {:?}",
        problem.module
    );
    assert!(
        problem.note.as_deref().is_some_and(|note| !note.is_empty()),
        "it says what to do"
    );
    assert!(
        pipelines.overlays().is_empty(),
        "the overlay is not used: {:?}",
        pipelines.overlays()
    );
}

// ─── A pipeline system and a device that do nothing ─────────────────────────

/// A pipeline system that records the overlays it is handed and the times
/// they are cleared, holds the overlays set since the last clear, and builds
/// nothing.
#[derive(Default)]
pub(super) struct RecordingPipelines {
    overlays: Mutex<Vec<(String, String)>>,
    held: Mutex<std::collections::BTreeMap<String, String>>,
    clears: std::sync::atomic::AtomicUsize,
}

impl RecordingPipelines {
    /// Every overlay handed to it, in order, cleared or not.
    pub(super) fn overlays(&self) -> Vec<(String, String)> {
        self.overlays.lock().expect("the overlay lock").clone()
    }

    /// The overlays it holds now: each module's last source set since the
    /// last clear.
    pub(super) fn held(&self) -> std::collections::BTreeMap<String, String> {
        self.held.lock().expect("the held lock").clone()
    }

    /// How many times the overlays were cleared.
    pub(super) fn clears(&self) -> usize {
        self.clears.load(std::sync::atomic::Ordering::SeqCst)
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
            .push((logical_path.to_owned(), source.clone()));
        self.held
            .lock()
            .expect("the held lock")
            .insert(logical_path.to_owned(), source);
    }
    fn clear_overlays(&self) {
        self.held.lock().expect("the held lock").clear();
        self.clears
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
    fn recompose_dirty(&self, _device: &dyn GraphicsDevice) -> Result<(), RenderError> {
        Ok(())
    }
}

/// A device the shader pump only hands on to `recompose_dirty`: it is never
/// asked to do anything.
#[derive(Debug)]
pub(super) struct UnusedDevice;

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

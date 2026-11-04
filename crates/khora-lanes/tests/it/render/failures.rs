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

//! What a render lane does when the device or the pipeline system refuses
//! something it asks for: the frame fails, and whatever the lane built before
//! the refusal is kept for the retry or given back — never leaked, never
//! destroyed twice. A material variant whose pipeline is refused leaves the
//! rest of the frame drawn.

use std::collections::HashSet;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use khora_core::gpu::GpuSetup;
use khora_core::lane::erased::{AnyLane, Run, RunOutcome};
use khora_core::lane::io::{FrameId, InputSource, OutputSink};
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::math::dimension::{Extent3D, Origin3D};
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
use khora_core::renderer::api::shadow::ShadowFrame;
use khora_core::renderer::error::{RenderError, ResourceError};
use khora_core::renderer::traits::{CommandEncoder, PipelineSystem};
use khora_core::renderer::GraphicsDevice;
use khora_data::render::ScenePassSlot;

use super::fixtures::{
    LedgerPipelines, RenderLedger, Scene, Surface, EVERY_LANE, LEVEL, LIT_LANES,
};

// ─── A device that refuses one creation ─────────────────────────────────────

/// The ledger, refusing the `refuse_at`-th object a lane asks it to create
/// (buffers, textures, views, samplers, bind groups — what a lane owns),
/// counting from 1; 0 refuses nothing.
#[derive(Debug, Default)]
struct Refusing {
    ledger: RenderLedger,
    refuse_at: AtomicUsize,
    creates: AtomicUsize,
}

impl Refusing {
    fn refusing_at(n: usize) -> Self {
        let device = Self::default();
        device.refuse_at.store(n, Ordering::SeqCst);
        device
    }

    fn gate(&self) -> Result<(), ResourceError> {
        let n = self.creates.fetch_add(1, Ordering::SeqCst) + 1;
        if n == self.refuse_at.load(Ordering::SeqCst) {
            Err(ResourceError::BackendError(format!("creation {n} refused")))
        } else {
            Ok(())
        }
    }
}

impl GraphicsDevice for Refusing {
    fn create_shader_module(
        &self,
        d: &ShaderModuleDescriptor,
    ) -> Result<ShaderModuleId, ResourceError> {
        self.ledger.create_shader_module(d)
    }
    fn destroy_shader_module(&self, id: ShaderModuleId) -> Result<(), ResourceError> {
        self.ledger.destroy_shader_module(id)
    }
    fn create_render_pipeline(
        &self,
        d: &RenderPipelineDescriptor,
    ) -> Result<RenderPipelineId, ResourceError> {
        self.ledger.create_render_pipeline(d)
    }
    fn create_pipeline_layout(
        &self,
        d: &PipelineLayoutDescriptor,
    ) -> Result<PipelineLayoutId, ResourceError> {
        self.ledger.create_pipeline_layout(d)
    }
    fn destroy_render_pipeline(&self, id: RenderPipelineId) -> Result<(), ResourceError> {
        self.ledger.destroy_render_pipeline(id)
    }
    fn create_compute_pipeline(
        &self,
        d: &ComputePipelineDescriptor,
    ) -> Result<ComputePipelineId, ResourceError> {
        self.ledger.create_compute_pipeline(d)
    }
    fn destroy_compute_pipeline(&self, id: ComputePipelineId) -> Result<(), ResourceError> {
        self.ledger.destroy_compute_pipeline(id)
    }
    fn create_bind_group_layout(
        &self,
        d: &BindGroupLayoutDescriptor,
    ) -> Result<BindGroupLayoutId, ResourceError> {
        self.ledger.create_bind_group_layout(d)
    }
    fn create_bind_group(&self, d: &BindGroupDescriptor) -> Result<BindGroupId, ResourceError> {
        self.gate()?;
        self.ledger.create_bind_group(d)
    }
    fn destroy_bind_group_layout(&self, id: BindGroupLayoutId) -> Result<(), ResourceError> {
        self.ledger.destroy_bind_group_layout(id)
    }
    fn destroy_bind_group(&self, id: BindGroupId) -> Result<(), ResourceError> {
        self.ledger.destroy_bind_group(id)
    }
    fn create_buffer(&self, d: &BufferDescriptor) -> Result<BufferId, ResourceError> {
        self.gate()?;
        self.ledger.create_buffer(d)
    }
    fn create_buffer_with_data(
        &self,
        d: &BufferDescriptor,
        data: &[u8],
    ) -> Result<BufferId, ResourceError> {
        self.gate()?;
        self.ledger.create_buffer_with_data(d, data)
    }
    fn destroy_buffer(&self, id: BufferId) -> Result<(), ResourceError> {
        self.ledger.destroy_buffer(id)
    }
    fn write_buffer(&self, id: BufferId, offset: u64, data: &[u8]) -> Result<(), ResourceError> {
        self.ledger.write_buffer(id, offset, data)
    }
    fn write_buffer_async<'a>(
        &'a self,
        id: BufferId,
        offset: u64,
        data: &'a [u8],
    ) -> Box<dyn Future<Output = Result<(), ResourceError>> + Send + 'static> {
        self.ledger.write_buffer_async(id, offset, data)
    }
    fn create_texture(&self, d: &TextureDescriptor) -> Result<TextureId, ResourceError> {
        self.gate()?;
        self.ledger.create_texture(d)
    }
    fn destroy_texture(&self, id: TextureId) -> Result<(), ResourceError> {
        self.ledger.destroy_texture(id)
    }
    fn write_texture(
        &self,
        texture_id: TextureId,
        data: &[u8],
        bytes_per_row: Option<u32>,
        offset: Origin3D,
        size: Extent3D,
    ) -> Result<(), ResourceError> {
        self.ledger
            .write_texture(texture_id, data, bytes_per_row, offset, size)
    }
    fn create_texture_view(
        &self,
        texture_id: TextureId,
        d: &TextureViewDescriptor,
    ) -> Result<TextureViewId, ResourceError> {
        self.gate()?;
        self.ledger.create_texture_view(texture_id, d)
    }
    fn destroy_texture_view(&self, id: TextureViewId) -> Result<(), ResourceError> {
        self.ledger.destroy_texture_view(id)
    }
    fn create_sampler(&self, d: &SamplerDescriptor) -> Result<SamplerId, ResourceError> {
        self.gate()?;
        self.ledger.create_sampler(d)
    }
    fn destroy_sampler(&self, id: SamplerId) -> Result<(), ResourceError> {
        self.ledger.destroy_sampler(id)
    }
    fn create_command_encoder(&self, label: Option<&str>) -> Box<dyn CommandEncoder> {
        self.ledger.create_command_encoder(label)
    }
    fn submit_command_buffer(&self, command_buffer: CommandBufferId) {
        self.ledger.submit_command_buffer(command_buffer)
    }
    fn get_surface_format(&self) -> Option<TextureFormat> {
        self.ledger.get_surface_format()
    }
    fn get_surface_size(&self) -> (u32, u32) {
        self.ledger.get_surface_size()
    }
    fn get_adapter_info(&self) -> GraphicsAdapterInfo {
        self.ledger.get_adapter_info()
    }
    fn supports_feature(&self, feature_name: &str) -> bool {
        self.ledger.supports_feature(feature_name)
    }
}

// ─── A pipeline system that refuses some pipelines ──────────────────────────

/// The ledger's pipeline system, refusing every blended render pipeline
/// while `refuses_blended`, and every render pipeline while `refuses_all`.
#[derive(Debug, Default)]
struct Picky {
    pipelines: LedgerPipelines,
    refuses_blended: AtomicBool,
    refuses_all: AtomicBool,
}

impl PipelineSystem for Picky {
    fn layout(
        &self,
        device: &dyn GraphicsDevice,
        key: LayoutKey,
        variant: &ShaderVariantKey,
    ) -> Result<BindGroupLayoutId, RenderError> {
        self.pipelines.layout(device, key, variant)
    }
    fn inline_layout(
        &self,
        device: &dyn GraphicsDevice,
        label: &'static str,
        entries: &[BindGroupLayoutEntry],
    ) -> Result<BindGroupLayoutId, RenderError> {
        self.pipelines.inline_layout(device, label, entries)
    }
    fn pipeline(
        &self,
        device: &dyn GraphicsDevice,
        spec: &PipelineSpec,
    ) -> Result<RenderPipelineId, RenderError> {
        let blended = spec.color_targets.iter().any(|t| t.blend.is_some());
        if self.refuses_all.load(Ordering::SeqCst)
            || (blended && self.refuses_blended.load(Ordering::SeqCst))
        {
            return Err(RenderError::Internal("pipeline refused".into()));
        }
        self.pipelines.pipeline(device, spec)
    }
    fn compute_pipeline(
        &self,
        device: &dyn GraphicsDevice,
        spec: &ComputePipelineSpec,
    ) -> Result<ComputePipelineId, RenderError> {
        self.pipelines.compute_pipeline(device, spec)
    }
}

// ─── Running a lane on them ─────────────────────────────────────────────────

/// What one frame gave back: the run, and the draws of its scene pass.
struct Frame {
    run: Run,
    scene_pass: Option<CommandBufferId>,
}

/// Records frame `frame` of `lane` over `scene`, on `device` and `pipelines`,
/// as the agent's layer does.
fn record(
    lane: &mut Box<dyn AnyLane>,
    device: &dyn GraphicsDevice,
    pipelines: &dyn PipelineSystem,
    scene: &Scene,
    frame: u64,
) -> Frame {
    let id = FrameId(frame);
    let mut bus = LaneBus::new();
    bus.set_frame(id);
    bus.publish(scene.render_world.clone());
    let mut earlier = OutputDeck::new();
    if let Some(shadow) = &scene.shadow {
        *earlier.slot::<ShadowFrame>() = shadow.clone();
    }
    let mut deck = OutputDeck::new();
    let run = {
        let mut inputs = InputSource::new(id, &bus, &scene.runtime)
            .with_deck(&earlier)
            .with_clear_color(scene.clear);
        if let Some(target) = scene.target {
            inputs = inputs.with_target(target);
        }
        let mut outputs = OutputSink::new(&mut deck)
            .with_device(device)
            .with_pipelines(pipelines);
        let mut gpu = GpuSetup::new(device).with_pipelines(pipelines);
        lane.run(LEVEL, &mut inputs, &mut outputs, &mut gpu)
    };
    let scene_pass = deck
        .contains::<ScenePassSlot>()
        .then(|| deck.take::<ScenePassSlot>().0)
        .flatten()
        .map(|pass| pass.command_buffer);
    Frame { run, scene_pass }
}

fn shut_down(
    lane: &mut Box<dyn AnyLane>,
    device: &dyn GraphicsDevice,
    pipelines: &dyn PipelineSystem,
) {
    let mut gpu = GpuSetup::new(device).with_pipelines(pipelines);
    lane.shutdown(&mut gpu);
}

/// A frame every lane draws something in: a camera, an opaque and a blended
/// mesh, a light of each kind, a shadow frame.
fn busy_scene() -> Scene {
    Scene::new()
        .with_mesh(Surface::OPAQUE, 2.0)
        .with_mesh(Surface::BLENDED, 4.0)
        .with_suns(1)
        .with_lamps(1)
        .with_spots(1)
        .with_shadow_frame()
}

/// What the lane made on `ledger` and never gave back, and what it gave
/// back more than once.
fn leaks_and_doubles(ledger: &RenderLedger) -> (Vec<String>, Vec<String>) {
    let destroyed = ledger.destroyed();
    let leaked = ledger
        .lane_made()
        .into_iter()
        .filter(|made| !destroyed.contains(&(made.kind, made.id)))
        .map(|made| format!("{:?} {} `{}`", made.kind, made.id, made.label))
        .collect();
    let mut seen = HashSet::new();
    let twice = destroyed
        .iter()
        .filter(|entry| !seen.insert(**entry))
        .map(|entry| format!("{entry:?}"))
        .collect();
    (leaked, twice)
}

// ─── The device refusing the Nth creation ───────────────────────────────────

/// **A device refusing any one creation of a lane's first frame leaks
/// nothing.** For each lane, the first frame is run once on a device that
/// refuses its `n`-th creation, for every `n` the frame makes, then once
/// more on a device that refuses nothing, then shut down: every object the
/// lane made is given back exactly once.
#[test]
fn a_refused_creation_on_the_first_frame_leaks_nothing() {
    let scene = busy_scene();
    let mut failures = Vec::new();
    for which in EVERY_LANE {
        // How many objects a clean first frame makes.
        let creations = {
            let device = Refusing::default();
            let pipelines = Picky::default();
            let mut lane = which.lane();
            let frame = record(&mut lane, &device, &pipelines, &scene, 1);
            assert!(
                matches!(frame.run.outcome, RunOutcome::Recorded(_)),
                "{which:?}: a clean first frame records: {:?}",
                frame.run.outcome
            );
            shut_down(&mut lane, &device, &pipelines);
            device.creates.load(Ordering::SeqCst)
        };
        assert!(
            creations > 0,
            "{which:?} makes something on its first frame"
        );

        for n in 1..=creations {
            let device = Refusing::refusing_at(n);
            let pipelines = Picky::default();
            let mut lane = which.lane();
            let refused = record(&mut lane, &device, &pipelines, &scene, 1);
            assert!(
                matches!(refused.run.outcome, RunOutcome::Failed(_)),
                "{which:?}: refusing creation {n} fails the frame: {:?}",
                refused.run.outcome
            );
            device.refuse_at.store(0, Ordering::SeqCst);
            record(&mut lane, &device, &pipelines, &scene, 2);
            shut_down(&mut lane, &device, &pipelines);

            let (leaked, twice) = leaks_and_doubles(&device.ledger);
            if !leaked.is_empty() || !twice.is_empty() {
                failures.push(format!(
                    "{which:?}, creation {n} of {creations} refused: leaked {leaked:?}, destroyed twice {twice:?}"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// ─── The pipeline system refusing ───────────────────────────────────────────

/// **A refused pipeline at prepare fails the frame and keeps what was built
/// for the retry.** The pipeline system refuses every render pipeline for
/// two frames: both fail, committing no scene pass. Once it answers, the
/// next frame records, and the lane made no more device objects across the
/// refusals than one clean first frame makes.
#[test]
fn a_refused_pipeline_at_prepare_keeps_what_was_built_for_the_retry() {
    let scene = busy_scene();
    for which in EVERY_LANE {
        let clean = {
            let device = Refusing::default();
            let pipelines = Picky::default();
            let mut lane = which.lane();
            record(&mut lane, &device, &pipelines, &scene, 1);
            device.ledger.lane_made().len()
        };

        let device = Refusing::default();
        let pipelines = Picky::default();
        pipelines.refuses_all.store(true, Ordering::SeqCst);
        let mut lane = which.lane();
        for frame in 1..=2 {
            let refused = record(&mut lane, &device, &pipelines, &scene, frame);
            assert!(
                matches!(refused.run.outcome, RunOutcome::Failed(_)),
                "{which:?}: a refused pipeline fails the frame: {:?}",
                refused.run.outcome
            );
            assert!(
                refused.scene_pass.is_none(),
                "{which:?}: a failed frame commits no scene pass"
            );
        }
        pipelines.refuses_all.store(false, Ordering::SeqCst);
        let retried = record(&mut lane, &device, &pipelines, &scene, 3);
        assert!(
            matches!(retried.run.outcome, RunOutcome::Recorded(_)),
            "{which:?}: the retry records: {:?}",
            retried.run.outcome
        );
        assert_eq!(
            device.ledger.lane_made().len(),
            clean,
            "{which:?}: the refusals built nothing twice"
        );
        shut_down(&mut lane, &device, &pipelines);
        let (leaked, twice) = leaks_and_doubles(&device.ledger);
        assert!(leaked.is_empty(), "{which:?}: leaked {leaked:?}");
        assert!(twice.is_empty(), "{which:?}: destroyed twice {twice:?}");
    }
}

/// **A material variant whose pipeline is refused leaves the rest of the
/// frame drawn.** The pipeline system refuses blended pipelines: a lit
/// lane's frame over an opaque and a blended mesh still records, drawing the
/// opaque mesh in its scene pass.
#[test]
fn a_refused_variant_pipeline_leaves_the_rest_of_the_frame_drawn() {
    let scene = busy_scene();
    for which in LIT_LANES {
        let device = Refusing::default();
        let pipelines = Picky::default();
        pipelines.refuses_blended.store(true, Ordering::SeqCst);
        let mut lane = which.lane();

        let frame = record(&mut lane, &device, &pipelines, &scene, 1);
        assert!(
            matches!(frame.run.outcome, RunOutcome::Recorded(_)),
            "{which:?}: the frame records: {:?}",
            frame.run.outcome
        );
        let scene_pass = frame.scene_pass.expect("the scene pass is committed");
        let draws: usize = device
            .ledger
            .passes_into(scene_pass)
            .iter()
            .map(|pass| pass.draws.len())
            .sum();
        assert_eq!(draws, 1, "{which:?}: the opaque mesh is drawn");
        shut_down(&mut lane, &device, &pipelines);
    }
}

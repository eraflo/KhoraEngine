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

//! The device called from many threads at once, on a real adapter.
//!
//! Lanes create, grow, write and release resources inside concurrent waves,
//! so every `GraphicsDevice` and `PipelineSystem` method may be entered by
//! several threads at the same time. A device whose locks are taken in
//! conflicting orders (one method takes A then B, another B then A) deadlocks
//! there. These tests hammer the device from eight threads in shuffled orders
//! and fail, instead of hanging, if the threads have not all come back within
//! [`WATCHDOG`].
//!
//! # Why a panic and not an abort
//!
//! On a deadlock the test thread panics with the names of the threads still
//! stuck and leaves them detached. Under libtest the test is reported failed
//! with that message and the other tests of the binary still run and report;
//! the binary then exits, and exiting a Rust process does not wait for
//! detached threads. Under nextest each test is its own process, so the panic
//! fails exactly that test. `std::process::abort` would instead take down
//! every test of a libtest binary with it and show up as a crash rather than a
//! failure carrying the message.
//!
//! Without any adapter (a CI runner without a GPU) each test logs why and
//! returns.

use std::borrow::Cow;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use khora_core::math::{Extent3D, Origin3D};
use khora_core::renderer::api::command::{
    BindGroupDescriptor, BindGroupEntry, BindGroupId, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindGroupLayoutId, BindingResource, BindingType, BufferBindingType,
    SamplerBindingType, TextureSampleType,
};
use khora_core::renderer::api::pipeline::enums::{
    BlendFactor, BlendOperation, CompareFunction, CullMode, PrimitiveTopology,
};
use khora_core::renderer::api::pipeline::state::{
    BlendComponentDescriptor, BlendStateDescriptor, ColorWrites, DepthBiasState, StencilFaceState,
};
use khora_core::renderer::api::pipeline::{
    ColorTargetStateDescriptor, DepthStencilStateDescriptor, LayoutSpec,
    MultisampleStateDescriptor, PipelineSpec, PrimitiveStateDescriptor,
};
use khora_core::renderer::api::resource::{
    AddressMode, BufferDescriptor, BufferId, BufferUsage, FilterMode, ImageAspect,
    MipmapFilterMode, SampleCount, SamplerDescriptor, SamplerId, TextureDescriptor,
    TextureDimension, TextureFormat, TextureId, TextureUsage, TextureViewDescriptor,
    TextureViewDimension, TextureViewId,
};
use khora_core::renderer::api::shader::ShaderVariantKey;
use khora_core::renderer::api::util::ShaderStageFlags;
use khora_core::renderer::traits::PipelineSystem;
use khora_core::renderer::GraphicsDevice;
use khora_core::telemetry::ResourceMonitor;

use super::super::WgpuDevice;
use crate::graphics::wgpu::pipeline_system::WgpuPipelineSystem;
use crate::graphics::wgpu::resilience::GpuHealth;

/// How long each thread keeps calling the device.
const WORK: Duration = Duration::from_millis(500);
/// How long the test waits for every thread before it calls it a deadlock.
const WATCHDOG: Duration = Duration::from_secs(10);
/// Threads calling the device at once.
const THREADS: usize = 8;
/// Resources of one kind a thread keeps alive before it gives the oldest back.
const POOL: usize = 8;

const BUFFER_SIZE: u64 = 256;
const TEXTURE_SIDE: u32 = 16;

// ─── A real device, or none ─────────────────────────────────────────────────

/// A wgpu device and queue, plus every error wgpu reported on them, and the
/// test's turn on the GPU — declared last, so it is given back after the
/// device is dropped.
struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    errors: Arc<Mutex<Vec<String>>>,
    _turn: std::sync::MutexGuard<'static, ()>,
}

impl Gpu {
    /// The validation and internal errors wgpu reported, if any.
    fn reported_errors(&self) -> Vec<String> {
        self.errors
            .lock()
            .map(|errors| errors.clone())
            .unwrap_or_else(|_| vec!["the error list was poisoned".to_owned()])
    }
}

/// A device of its own for `test`, or `None` (logged) without any adapter.
fn gpu(test: &str) -> Option<Gpu> {
    let turn = crate::graphics::wgpu::tests::gpu::exclusive_gpu();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = [false, true]
        .into_iter()
        .find_map(|force_fallback_adapter| {
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: None,
                force_fallback_adapter,
            }))
            .ok()
        });
    let Some(adapter) = adapter else {
        log::warn!("{test}: no wgpu adapter, skipped");
        return None;
    };
    let (device, queue) =
        match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some(test),
            required_features: wgpu::Features::empty(),
            required_limits: adapter.limits(),
            experimental_features: wgpu::ExperimentalFeatures::default(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        })) {
            Ok(pair) => pair,
            Err(e) => {
                log::warn!("{test}: no device ({e}), skipped");
                return None;
            }
        };

    // An invalid call must show up as a failure of the test, not as a panic
    // on whichever thread wgpu happens to report it on.
    let errors = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&errors);
    device.on_uncaptured_error(Arc::new(move |e| {
        if let Ok(mut errors) = sink.lock() {
            errors.push(format!("{e:?}"));
        }
    }));

    Some(Gpu {
        device,
        queue,
        errors,
        _turn: turn,
    })
}

/// The device under test, built without any surface.
fn headless_device(gpu: &Gpu) -> WgpuDevice {
    WgpuDevice::new(
        gpu.device.clone(),
        gpu.queue.clone(),
        Arc::new(GpuHealth::default()),
    )
}

// ─── Seeded shuffling, no dependency ────────────────────────────────────────

/// xorshift64: enough to shuffle calls reproducibly per thread.
struct XorShift(u64);

impl XorShift {
    fn seeded(thread: usize) -> Self {
        // Never zero: xorshift stays at zero forever.
        Self(0x9E37_79B9_7F4A_7C15 ^ (thread as u64 + 1).wrapping_mul(0xD1B5_4A32_D192_ED03))
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

// ─── The watchdog ───────────────────────────────────────────────────────────

type Worker = Box<dyn FnOnce() -> Result<(), String> + Send + 'static>;

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "a panic without a message".to_owned()
    }
}

/// Runs every worker on a thread of its own and waits for all of them for at
/// most [`WATCHDOG`]. Fails with the stuck threads' names on a timeout (they
/// stay detached), and with each worker's error or panic otherwise.
fn run_watched(what: &str, workers: Vec<(String, Worker)>) {
    let (sender, receiver) = mpsc::channel::<(usize, Result<(), String>)>();
    let names: Vec<String> = workers.iter().map(|(name, _)| name.clone()).collect();

    for (index, (name, work)) in workers.into_iter().enumerate() {
        let sender = sender.clone();
        // The handle is dropped: a thread stuck in a deadlock stays detached
        // and cannot keep the test from reporting.
        let _detached = std::thread::Builder::new()
            .name(name)
            .spawn(move || {
                let outcome = catch_unwind(AssertUnwindSafe(work))
                    .unwrap_or_else(|payload| Err(format!("panicked: {}", panic_message(payload))));
                let _ = sender.send((index, outcome));
            })
            .expect("a test thread spawns");
    }
    drop(sender);

    let deadline = Instant::now() + WATCHDOG;
    let mut finished = vec![false; names.len()];
    let mut failures = Vec::new();
    while finished.iter().any(|done| !done) {
        let left = deadline.saturating_duration_since(Instant::now());
        match receiver.recv_timeout(left) {
            Ok((index, outcome)) => {
                finished[index] = true;
                if let Err(message) = outcome {
                    failures.push(format!("{}: {message}", names[index]));
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                let stuck: Vec<&str> = names
                    .iter()
                    .zip(&finished)
                    .filter(|(_, done)| !**done)
                    .map(|(name, _)| name.as_str())
                    .collect();
                panic!(
                    "deadlock in {what}: {} of {} threads had not come back {WATCHDOG:?} after \
                     they started, though each stops calling after {WORK:?} — stuck: {stuck:?}. \
                     Two of the device's locks are taken in conflicting orders.",
                    stuck.len(),
                    names.len(),
                );
            }
            Err(RecvTimeoutError::Disconnected) => {
                panic!("{what}: a worker thread ended without reporting");
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{what}: calls failed from many threads at once:\n{}",
        failures.join("\n")
    );
}

// ─── Descriptors ────────────────────────────────────────────────────────────

fn buffer_descriptor() -> BufferDescriptor<'static> {
    BufferDescriptor {
        label: Some(Cow::Borrowed("concurrency buffer")),
        size: BUFFER_SIZE,
        usage: BufferUsage::UNIFORM | BufferUsage::COPY_DST | BufferUsage::COPY_SRC,
        mapped_at_creation: false,
    }
}

fn texture_descriptor() -> TextureDescriptor<'static> {
    TextureDescriptor {
        label: Some(Cow::Borrowed("concurrency texture")),
        size: Extent3D {
            width: TEXTURE_SIDE,
            height: TEXTURE_SIDE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: SampleCount::X1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8Unorm,
        usage: TextureUsage::TEXTURE_BINDING | TextureUsage::COPY_DST,
        view_formats: Cow::Borrowed(&[]),
    }
}

fn view_descriptor() -> TextureViewDescriptor<'static> {
    TextureViewDescriptor {
        label: Some(Cow::Borrowed("concurrency view")),
        format: None,
        dimension: Some(TextureViewDimension::D2),
        aspect: ImageAspect::All,
        base_mip_level: 0,
        mip_level_count: None,
        base_array_layer: 0,
        array_layer_count: None,
    }
}

fn sampler_descriptor() -> SamplerDescriptor<'static> {
    SamplerDescriptor {
        label: Some(Cow::Borrowed("concurrency sampler")),
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        address_mode_w: AddressMode::ClampToEdge,
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        mipmap_filter: MipmapFilterMode::Linear,
        lod_min_clamp: 0.0,
        lod_max_clamp: 100.0,
        compare: None,
        anisotropy_clamp: 1,
        border_color: None,
    }
}

/// A uniform buffer, a filterable 2D texture and its sampler, seen by the
/// fragment stage.
fn material_layout_entries() -> Vec<BindGroupLayoutEntry> {
    vec![
        BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStageFlags::FRAGMENT,
            ty: BindingType::Buffer {
                ty: BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
        },
        BindGroupLayoutEntry {
            binding: 1,
            visibility: ShaderStageFlags::FRAGMENT,
            ty: BindingType::Texture {
                sample_type: TextureSampleType::Float { filterable: true },
                view_dimension: TextureViewDimension::D2,
                multisampled: false,
            },
        },
        BindGroupLayoutEntry {
            binding: 2,
            visibility: ShaderStageFlags::FRAGMENT,
            ty: BindingType::Sampler(SamplerBindingType::Filtering),
        },
    ]
}

/// The grid lane's camera layout: one uniform buffer, vertex + fragment.
fn grid_camera_layout_entries() -> Vec<BindGroupLayoutEntry> {
    vec![BindGroupLayoutEntry {
        binding: 0,
        visibility: ShaderStageFlags::VERTEX | ShaderStageFlags::FRAGMENT,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
    }]
}

const GRID_CAMERA_LAYOUT_LABEL: &str = "grid_camera_layout";

/// The grid lane's pipeline spec (`khora-lanes` `overlay_lane/grid.rs`,
/// `grid_pipeline_spec`), for a target of `color` and `depth` formats with
/// `samples` per pixel, culling `cull`.
fn grid_pipeline_spec(
    color: TextureFormat,
    depth: TextureFormat,
    samples: SampleCount,
    cull: Option<CullMode>,
) -> PipelineSpec {
    let blend = BlendStateDescriptor {
        color: BlendComponentDescriptor {
            src_factor: BlendFactor::SrcAlpha,
            dst_factor: BlendFactor::OneMinusSrcAlpha,
            operation: BlendOperation::Add,
        },
        alpha: BlendComponentDescriptor {
            src_factor: BlendFactor::One,
            dst_factor: BlendFactor::OneMinusSrcAlpha,
            operation: BlendOperation::Add,
        },
    };
    PipelineSpec {
        label: "Grid Pipeline",
        shader: "khora::pipelines::grid",
        variant: ShaderVariantKey::empty(),
        bind_group_layouts: vec![LayoutSpec::Inline {
            label: GRID_CAMERA_LAYOUT_LABEL,
            entries: Cow::Owned(grid_camera_layout_entries()),
        }],
        vertex_buffers: vec![],
        vs_entry: "vs_main",
        fs_entry: Some("fs_main"),
        primitive: PrimitiveStateDescriptor {
            topology: PrimitiveTopology::TriangleList,
            cull_mode: cull,
            ..Default::default()
        },
        depth_stencil: Some(DepthStencilStateDescriptor {
            format: depth,
            depth_write_enabled: false,
            depth_compare: CompareFunction::LessEqual,
            stencil_front: StencilFaceState::default(),
            stencil_back: StencilFaceState::default(),
            stencil_read_mask: 0,
            stencil_write_mask: 0,
            bias: DepthBiasState::default(),
        }),
        color_targets: vec![ColorTargetStateDescriptor {
            format: color,
            blend: Some(blend),
            write_mask: ColorWrites::ALL,
        }],
        multisample: MultisampleStateDescriptor {
            count: samples,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
    }
}

/// One of 48 grid pipelines: four colour formats, two depth formats, two
/// sample counts, three cull modes.
fn some_grid_pipeline(rng: &mut XorShift) -> PipelineSpec {
    const COLORS: [TextureFormat; 4] = [
        TextureFormat::Rgba8Unorm,
        TextureFormat::Rgba8UnormSrgb,
        TextureFormat::Bgra8UnormSrgb,
        TextureFormat::Rgba16Float,
    ];
    const DEPTHS: [TextureFormat; 2] = [TextureFormat::Depth32Float, TextureFormat::Depth24Plus];
    const SAMPLES: [SampleCount; 2] = [SampleCount::X1, SampleCount::X4];
    const CULLS: [Option<CullMode>; 3] = [None, Some(CullMode::Back), Some(CullMode::Front)];
    grid_pipeline_spec(
        COLORS[rng.below(COLORS.len())],
        DEPTHS[rng.below(DEPTHS.len())],
        SAMPLES[rng.below(SAMPLES.len())],
        CULLS[rng.below(CULLS.len())],
    )
}

// ─── One thread's share of the device ───────────────────────────────────────

/// Every call a thread makes, in the order [`Resources::call`] numbers them.
const CALLS: [&str; 15] = [
    "create_buffer",
    "write_buffer",
    "destroy_buffer",
    "create_texture",
    "write_texture",
    "destroy_texture",
    "create_texture_view",
    "destroy_texture_view",
    "create_sampler",
    "destroy_sampler",
    "create_bind_group_layout",
    "destroy_bind_group_layout",
    "create_bind_group",
    "destroy_bind_group",
    "encode_and_submit",
];

/// What one thread made and has not given back yet. A thread only touches
/// its own resources, so every call it makes is valid whatever the others do.
#[derive(Default)]
struct Resources {
    buffers: Vec<BufferId>,
    textures: Vec<TextureId>,
    /// Each view with the texture it was made from.
    views: Vec<(TextureViewId, TextureId)>,
    samplers: Vec<SamplerId>,
    layouts: Vec<BindGroupLayoutId>,
    groups: Vec<BindGroupId>,
    /// How many times each of [`CALLS`] ran.
    tally: [u32; CALLS.len()],
}

fn failed(call: &str, error: impl std::fmt::Debug) -> String {
    format!("{call} failed: {error:?}")
}

impl Resources {
    /// Makes call number `which` of [`CALLS`] (or the nearest one that has
    /// what it needs), through the `GraphicsDevice` trait.
    fn call(
        &mut self,
        device: &dyn GraphicsDevice,
        which: usize,
        rng: &mut XorShift,
    ) -> Result<(), String> {
        let ran = match which {
            0 => self.create_buffer(device)?,
            1 if !self.buffers.is_empty() => {
                let id = self.buffers[rng.below(self.buffers.len())];
                let offset = 64 * rng.below(4) as u64;
                let bytes = [rng.next() as u8; 64];
                device
                    .write_buffer(id, offset, &bytes)
                    .map_err(|e| failed("write_buffer", e))?;
                1
            }
            2 if !self.buffers.is_empty() => {
                let id = self.buffers.swap_remove(rng.below(self.buffers.len()));
                device
                    .destroy_buffer(id)
                    .map_err(|e| failed("destroy_buffer", e))?;
                2
            }
            3 => self.create_texture(device)?,
            4 if !self.textures.is_empty() => {
                let id = self.textures[rng.below(self.textures.len())];
                let texels = vec![rng.next() as u8; (TEXTURE_SIDE * TEXTURE_SIDE * 4) as usize];
                device
                    .write_texture(
                        id,
                        &texels,
                        Some(TEXTURE_SIDE * 4),
                        Origin3D::default(),
                        Extent3D {
                            width: TEXTURE_SIDE,
                            height: TEXTURE_SIDE,
                            depth_or_array_layers: 1,
                        },
                    )
                    .map_err(|e| failed("write_texture", e))?;
                4
            }
            5 if !self.textures.is_empty() => {
                let id = self.textures.swap_remove(rng.below(self.textures.len()));
                self.destroy_texture(device, id)?;
                5
            }
            6 if !self.textures.is_empty() => {
                self.create_view(device, rng)?;
                6
            }
            7 if !self.views.is_empty() => {
                let (view, _) = self.views.swap_remove(rng.below(self.views.len()));
                device
                    .destroy_texture_view(view)
                    .map_err(|e| failed("destroy_texture_view", e))?;
                7
            }
            8 => self.create_sampler(device)?,
            9 if !self.samplers.is_empty() => {
                let id = self.samplers.swap_remove(rng.below(self.samplers.len()));
                device
                    .destroy_sampler(id)
                    .map_err(|e| failed("destroy_sampler", e))?;
                9
            }
            10 => self.create_layout(device)?,
            11 if !self.layouts.is_empty() => {
                let id = self.layouts.swap_remove(rng.below(self.layouts.len()));
                device
                    .destroy_bind_group_layout(id)
                    .map_err(|e| failed("destroy_bind_group_layout", e))?;
                11
            }
            12 => self.create_bind_group(device, rng)?,
            13 if !self.groups.is_empty() => {
                let id = self.groups.swap_remove(rng.below(self.groups.len()));
                device
                    .destroy_bind_group(id)
                    .map_err(|e| failed("destroy_bind_group", e))?;
                13
            }
            14 => {
                self.encode_and_submit(device, rng)?;
                14
            }
            // A release or a write with nothing to act on makes the
            // matching resource instead.
            1 | 2 => self.create_buffer(device)?,
            4..=7 => self.create_texture(device)?,
            9 => self.create_sampler(device)?,
            11 => self.create_layout(device)?,
            13 => self.create_bind_group(device, rng)?,
            _ => unreachable!("{which} is not a call"),
        };
        self.tally[ran] += 1;
        Ok(())
    }

    fn create_buffer(&mut self, device: &dyn GraphicsDevice) -> Result<usize, String> {
        if self.buffers.len() >= POOL {
            let oldest = self.buffers.remove(0);
            device
                .destroy_buffer(oldest)
                .map_err(|e| failed("destroy_buffer", e))?;
            self.tally[2] += 1;
        }
        let id = device
            .create_buffer(&buffer_descriptor())
            .map_err(|e| failed("create_buffer", e))?;
        self.buffers.push(id);
        Ok(0)
    }

    fn create_texture(&mut self, device: &dyn GraphicsDevice) -> Result<usize, String> {
        if self.textures.len() >= POOL {
            let oldest = self.textures.remove(0);
            self.destroy_texture(device, oldest)?;
            self.tally[5] += 1;
        }
        let id = device
            .create_texture(&texture_descriptor())
            .map_err(|e| failed("create_texture", e))?;
        self.textures.push(id);
        Ok(3)
    }

    /// Gives back a texture and every view made from it.
    fn destroy_texture(
        &mut self,
        device: &dyn GraphicsDevice,
        id: TextureId,
    ) -> Result<(), String> {
        let mut index = 0;
        while index < self.views.len() {
            if self.views[index].1 == id {
                let (view, _) = self.views.swap_remove(index);
                device
                    .destroy_texture_view(view)
                    .map_err(|e| failed("destroy_texture_view", e))?;
                self.tally[7] += 1;
            } else {
                index += 1;
            }
        }
        device
            .destroy_texture(id)
            .map_err(|e| failed("destroy_texture", e))
    }

    fn create_view(
        &mut self,
        device: &dyn GraphicsDevice,
        rng: &mut XorShift,
    ) -> Result<(), String> {
        if self.views.len() >= POOL {
            let (oldest, _) = self.views.remove(0);
            device
                .destroy_texture_view(oldest)
                .map_err(|e| failed("destroy_texture_view", e))?;
            self.tally[7] += 1;
        }
        let texture = self.textures[rng.below(self.textures.len())];
        let view = device
            .create_texture_view(texture, &view_descriptor())
            .map_err(|e| failed("create_texture_view", e))?;
        self.views.push((view, texture));
        Ok(())
    }

    fn create_sampler(&mut self, device: &dyn GraphicsDevice) -> Result<usize, String> {
        if self.samplers.len() >= POOL {
            let oldest = self.samplers.remove(0);
            device
                .destroy_sampler(oldest)
                .map_err(|e| failed("destroy_sampler", e))?;
            self.tally[9] += 1;
        }
        let id = device
            .create_sampler(&sampler_descriptor())
            .map_err(|e| failed("create_sampler", e))?;
        self.samplers.push(id);
        Ok(8)
    }

    fn create_layout(&mut self, device: &dyn GraphicsDevice) -> Result<usize, String> {
        if self.layouts.len() >= POOL {
            let oldest = self.layouts.remove(0);
            device
                .destroy_bind_group_layout(oldest)
                .map_err(|e| failed("destroy_bind_group_layout", e))?;
            self.tally[11] += 1;
        }
        let entries = material_layout_entries();
        let id = device
            .create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: Some("concurrency layout"),
                entries: &entries,
            })
            .map_err(|e| failed("create_bind_group_layout", e))?;
        self.layouts.push(id);
        Ok(10)
    }

    /// Binds one of the thread's buffers, views and samplers to one of its
    /// layouts, making whichever of them it has none of yet.
    fn create_bind_group(
        &mut self,
        device: &dyn GraphicsDevice,
        rng: &mut XorShift,
    ) -> Result<usize, String> {
        if self.layouts.is_empty() {
            self.create_layout(device)?;
            self.tally[10] += 1;
        }
        if self.buffers.is_empty() {
            self.create_buffer(device)?;
            self.tally[0] += 1;
        }
        if self.textures.is_empty() {
            self.create_texture(device)?;
            self.tally[3] += 1;
        }
        if self.views.is_empty() {
            self.create_view(device, rng)?;
            self.tally[6] += 1;
        }
        if self.samplers.is_empty() {
            self.create_sampler(device)?;
            self.tally[8] += 1;
        }
        if self.groups.len() >= POOL {
            let oldest = self.groups.remove(0);
            device
                .destroy_bind_group(oldest)
                .map_err(|e| failed("destroy_bind_group", e))?;
            self.tally[13] += 1;
        }
        let layout = self.layouts[rng.below(self.layouts.len())];
        let buffer = self.buffers[rng.below(self.buffers.len())];
        let (view, _) = self.views[rng.below(self.views.len())];
        let sampler = self.samplers[rng.below(self.samplers.len())];
        let entries = [
            BindGroupEntry::buffer(0, buffer, 0, None),
            BindGroupEntry {
                binding: 1,
                resource: BindingResource::TextureView(view),
                _phantom: std::marker::PhantomData,
            },
            BindGroupEntry {
                binding: 2,
                resource: BindingResource::Sampler(sampler),
                _phantom: std::marker::PhantomData,
            },
        ];
        let id = device
            .create_bind_group(&BindGroupDescriptor {
                label: Some("concurrency bind group"),
                layout,
                entries: &entries,
            })
            .map_err(|e| failed("create_bind_group", e))?;
        self.groups.push(id);
        Ok(12)
    }

    /// Records a copy between two of the thread's buffers (when it has two),
    /// finishes the encoder and submits it.
    fn encode_and_submit(
        &mut self,
        device: &dyn GraphicsDevice,
        rng: &mut XorShift,
    ) -> Result<(), String> {
        let mut encoder = device.create_command_encoder(Some("concurrency encoder"));
        if self.buffers.len() >= 2 {
            let from = rng.below(self.buffers.len());
            let to = (from + 1 + rng.below(self.buffers.len() - 1)) % self.buffers.len();
            encoder.copy_buffer_to_buffer(&self.buffers[from], 0, &self.buffers[to], 64, 64);
        }
        let Some(commands) = encoder.finish() else {
            return Err("finish gave no command buffer".to_owned());
        };
        device.submit_command_buffer(commands);
        Ok(())
    }

    /// Gives back everything the thread still holds.
    fn release_all(&mut self, device: &dyn GraphicsDevice) -> Result<(), String> {
        for id in self.groups.drain(..) {
            device
                .destroy_bind_group(id)
                .map_err(|e| failed("destroy_bind_group", e))?;
        }
        for (view, _) in self.views.drain(..) {
            device
                .destroy_texture_view(view)
                .map_err(|e| failed("destroy_texture_view", e))?;
        }
        for id in self.textures.drain(..) {
            device
                .destroy_texture(id)
                .map_err(|e| failed("destroy_texture", e))?;
        }
        for id in self.samplers.drain(..) {
            device
                .destroy_sampler(id)
                .map_err(|e| failed("destroy_sampler", e))?;
        }
        for id in self.layouts.drain(..) {
            device
                .destroy_bind_group_layout(id)
                .map_err(|e| failed("destroy_bind_group_layout", e))?;
        }
        for id in self.buffers.drain(..) {
            device
                .destroy_buffer(id)
                .map_err(|e| failed("destroy_buffer", e))?;
        }
        Ok(())
    }
}

/// A thread that makes shuffled calls on `device` for [`WORK`], then gives
/// everything back. Fails if one call failed, or if one of [`CALLS`] never
/// ran (the mix would then test less than it claims).
fn resource_worker(device: Arc<dyn GraphicsDevice>, thread: usize) -> Worker {
    Box::new(move || {
        let mut rng = XorShift::seeded(thread);
        let mut resources = Resources::default();
        let start = Instant::now();
        while start.elapsed() < WORK {
            let which = rng.below(CALLS.len());
            resources.call(device.as_ref(), which, &mut rng)?;
        }
        resources.release_all(device.as_ref())?;
        let never: Vec<&str> = CALLS
            .iter()
            .zip(&resources.tally)
            .filter(|(_, n)| **n == 0)
            .map(|(name, _)| *name)
            .collect();
        if never.is_empty() {
            Ok(())
        } else {
            Err(format!("these calls never ran in {WORK:?}: {never:?}"))
        }
    })
}

// ─── The tests ──────────────────────────────────────────────────────────────

/// Eight threads create, write, bind, encode, submit and destroy buffers,
/// textures, views, samplers, layouts and bind groups in shuffled orders, all
/// on one device. They all come back, every call succeeds, wgpu reports no
/// error, and once every thread gave its resources back the device tracks no
/// memory.
#[test]
fn the_device_survives_every_call_from_many_threads_at_once() {
    let test = "the_device_survives_every_call_from_many_threads_at_once";
    let Some(gpu) = gpu(test) else {
        return;
    };
    let device = headless_device(&gpu);
    let shared: Arc<dyn GraphicsDevice> = Arc::new(device.clone());

    let workers = (0..THREADS)
        .map(|thread| {
            (
                format!("device-{thread}"),
                resource_worker(Arc::clone(&shared), thread),
            )
        })
        .collect();
    run_watched("the device", workers);

    assert_eq!(
        gpu.reported_errors(),
        Vec::<String>::new(),
        "wgpu reported errors"
    );
    assert_eq!(
        device.get_usage_report().current_bytes,
        0,
        "every thread gave back what it made, so the device holds no memory"
    );
}

/// Four threads build bind-group layouts and grid pipelines for 48 target
/// shapes through one shared `WgpuPipelineSystem`; one builds a fresh
/// pipeline system and its pipeline every turn (every lookup a miss); three
/// others create, write and destroy resources on the same device. They all
/// come back, every pipeline builds, and wgpu reports no error.
#[test]
fn pipelines_and_resources_built_from_many_threads_do_not_deadlock() {
    let test = "pipelines_and_resources_built_from_many_threads_do_not_deadlock";
    let Some(gpu) = gpu(test) else {
        return;
    };
    let device: Arc<dyn GraphicsDevice> = Arc::new(headless_device(&gpu));
    let pipelines = Arc::new(WgpuPipelineSystem::new().expect("the pipeline system composes"));

    let mut workers: Vec<(String, Worker)> = Vec::new();
    for thread in 0..4 {
        let device = Arc::clone(&device);
        let pipelines = Arc::clone(&pipelines);
        workers.push((
            format!("pipelines-{thread}"),
            Box::new(move || {
                let mut rng = XorShift::seeded(100 + thread);
                let start = Instant::now();
                while start.elapsed() < WORK {
                    if rng.below(3) == 0 {
                        pipelines
                            .inline_layout(
                                device.as_ref(),
                                GRID_CAMERA_LAYOUT_LABEL,
                                &grid_camera_layout_entries(),
                            )
                            .map_err(|e| failed("inline_layout", e))?;
                    } else {
                        pipelines
                            .pipeline(device.as_ref(), &some_grid_pipeline(&mut rng))
                            .map_err(|e| failed("pipeline", e))?;
                    }
                }
                Ok(())
            }),
        ));
    }
    {
        let device = Arc::clone(&device);
        workers.push((
            "fresh-pipelines".to_owned(),
            Box::new(move || {
                let mut rng = XorShift::seeded(200);
                let start = Instant::now();
                while start.elapsed() < WORK {
                    let fresh = WgpuPipelineSystem::new()
                        .map_err(|e| failed("WgpuPipelineSystem::new", e))?;
                    fresh
                        .pipeline(device.as_ref(), &some_grid_pipeline(&mut rng))
                        .map_err(|e| failed("pipeline", e))?;
                }
                Ok(())
            }),
        ));
    }
    for thread in 0..3 {
        workers.push((
            format!("resources-{thread}"),
            resource_worker(Arc::clone(&device), 300 + thread),
        ));
    }
    run_watched("the pipeline system and the device", workers);

    assert_eq!(
        gpu.reported_errors(),
        Vec::<String>::new(),
        "wgpu reported errors"
    );
}

/// A device built without a surface has no surface format and a zero size
/// until the render system tells it the surface's, and follows each new one.
#[test]
fn a_headless_device_reports_no_surface_until_told_one() {
    let test = "a_headless_device_reports_no_surface_until_told_one";
    let Some(gpu) = gpu(test) else {
        return;
    };
    let device = headless_device(&gpu);

    assert_eq!(device.get_surface_format(), None);
    assert_eq!(device.get_surface_size(), (0, 0));

    device.set_surface_format(Some(TextureFormat::Bgra8UnormSrgb), (1280, 720));
    assert_eq!(
        device.get_surface_format(),
        Some(TextureFormat::Bgra8UnormSrgb)
    );
    assert_eq!(device.get_surface_size(), (1280, 720));

    // A reconfigure (a resize, a new swapchain format) replaces both.
    device.set_surface_format(Some(TextureFormat::Rgba8UnormSrgb), (800, 600));
    assert_eq!(
        device.get_surface_format(),
        Some(TextureFormat::Rgba8UnormSrgb)
    );
    assert_eq!(device.get_surface_size(), (800, 600));
}

/// Rounds of concurrent submissions in
/// [`waiting_for_the_last_submission_waits_for_every_concurrent_submission`].
const SUBMIT_ROUNDS: usize = 1000;
/// Submissions each thread makes per round.
const SUBMITS_PER_THREAD: usize = 1;
/// The size of the copy every submission makes: large enough that a
/// submission is still running on the GPU when the one before it is done.
const SUBMIT_COPY_BYTES: u64 = 32 * 1024 * 1024;

/// Eight threads submit a copy each at once, round after round, while other
/// threads keep the registry busy (as a thread the OS preempts inside a lookup
/// does), so submitters come back from the queue in any order. Once a round
/// is over, `wait_for_last_submission` — what the render system calls before
/// the next acquire — returns only when every submission of the round is
/// done on the GPU, never after an earlier one alone.
#[test]
fn waiting_for_the_last_submission_waits_for_every_concurrent_submission() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    let test = "waiting_for_the_last_submission_waits_for_every_concurrent_submission";
    let Some(gpu) = gpu(test) else {
        return;
    };
    let device = headless_device(&gpu);
    let shared: Arc<dyn GraphicsDevice> = Arc::new(device.clone());

    let copy_buffer = |label: &'static str| {
        device
            .create_buffer(&BufferDescriptor {
                label: Some(Cow::Borrowed(label)),
                size: SUBMIT_COPY_BYTES,
                usage: BufferUsage::COPY_SRC | BufferUsage::COPY_DST,
                mapped_at_creation: false,
            })
            .expect("the copy buffers are created")
    };
    let (source, destination) = (copy_buffer("copy source"), copy_buffer("copy destination"));

    let stop = Arc::new(AtomicBool::new(false));
    let lookups: Vec<_> = (0..2)
        .map(|thread| {
            let device = device.clone();
            let stop = Arc::clone(&stop);
            std::thread::Builder::new()
                .name(format!("lookups-{thread}"))
                .spawn(move || {
                    let spin = |d: Duration| {
                        let from = Instant::now();
                        while from.elapsed() < d {
                            std::hint::spin_loop();
                        }
                    };
                    while !stop.load(Ordering::Relaxed) {
                        let _ = device.with_registry(|_| spin(Duration::from_micros(300)));
                        spin(Duration::from_micros(300));
                    }
                })
                .expect("a test thread spawns")
        })
        .collect();
    // Stops them on every way out, a failed round included.
    struct StopOnDrop(Arc<AtomicBool>);
    impl Drop for StopOnDrop {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Relaxed);
        }
    }
    let _stop_lookups = StopOnDrop(Arc::clone(&stop));

    let mut early = Vec::new();
    for round in 0..SUBMIT_ROUNDS {
        // How many submissions of this round the GPU has finished.
        let done = Arc::new(AtomicUsize::new(0));
        let start = Arc::new(std::sync::Barrier::new(THREADS));
        let workers = (0..THREADS)
            .map(|thread| {
                let device = Arc::clone(&shared);
                let queue = gpu.queue.clone();
                let done = Arc::clone(&done);
                let start = Arc::clone(&start);
                let work: Worker = Box::new(move || {
                    start.wait();
                    for _ in 0..SUBMITS_PER_THREAD {
                        let mut encoder = device.create_command_encoder(Some("submission"));
                        encoder.copy_buffer_to_buffer(
                            &source,
                            0,
                            &destination,
                            0,
                            SUBMIT_COPY_BYTES,
                        );
                        let Some(commands) = encoder.finish() else {
                            return Err("finish gave no command buffer".to_owned());
                        };
                        device.submit_command_buffer(commands);
                        // Fires once everything submitted so far — this
                        // submission included — is done.
                        let done = Arc::clone(&done);
                        queue.on_submitted_work_done(move || {
                            done.fetch_add(1, Ordering::SeqCst);
                        });
                    }
                    Ok(())
                });
                (format!("submit-{thread}"), work)
            })
            .collect();
        run_watched("concurrent submissions", workers);

        device.wait_for_last_submission();
        let finished = done.load(Ordering::SeqCst);
        if finished != THREADS * SUBMITS_PER_THREAD {
            early.push(format!(
                "round {round}: {finished} of {} submissions done",
                THREADS * SUBMITS_PER_THREAD
            ));
        }
    }
    stop.store(true, Ordering::Relaxed);
    for lookup in lookups {
        let _ = lookup.join();
    }
    device.poll_device_blocking();
    let _ = shared.destroy_buffer(source);
    let _ = shared.destroy_buffer(destination);
    assert!(
        early.is_empty(),
        "after {} of {SUBMIT_ROUNDS} rounds of concurrent submissions, waiting for the last \
         submission returned while some were still running:\n{}",
        early.len(),
        early.join("\n")
    );
    assert_eq!(
        gpu.reported_errors(),
        Vec::<String>::new(),
        "wgpu reported errors"
    );
}

/// Waiting for the last submission on a device that never submitted, or
/// twice after one submission, returns at once.
#[test]
fn waiting_for_the_last_submission_with_nothing_submitted_returns() {
    let test = "waiting_for_the_last_submission_with_nothing_submitted_returns";
    let Some(gpu) = gpu(test) else {
        return;
    };
    let device = headless_device(&gpu);
    run_watched(
        "waiting with nothing submitted",
        vec![(
            "wait".to_owned(),
            Box::new(move || {
                device.wait_for_last_submission();
                let encoder = device.create_command_encoder(Some("one submission"));
                let Some(commands) = encoder.finish() else {
                    return Err("finish gave no command buffer".to_owned());
                };
                device.submit_command_buffer(commands);
                device.wait_for_last_submission();
                device.wait_for_last_submission();
                Ok(())
            }),
        )],
    );
}

/// The render system asks the device it holds (`Arc<WgpuDevice>`) with method
/// syntax, as `WgpuRenderSystem::supports_feature` does. A device built
/// without `POLYGON_MODE_POINT` answers no to that feature, and no to a name
/// it does not know, whichever way it is asked.
#[test]
fn a_feature_the_device_lacks_is_unsupported_whichever_way_it_is_asked() {
    let test = "a_feature_the_device_lacks_is_unsupported_whichever_way_it_is_asked";
    let Some(gpu) = gpu(test) else {
        return;
    };
    assert!(
        !gpu.device
            .features()
            .contains(wgpu::Features::POLYGON_MODE_POINT),
        "the test device is built with no features"
    );
    let held = Arc::new(headless_device(&gpu));

    assert!(
        !GraphicsDevice::supports_feature(held.as_ref(), "polygon_mode_point"),
        "through the trait"
    );
    assert!(
        !held.supports_feature("polygon_mode_point"),
        "through the device the render system holds"
    );
    assert!(
        !held.supports_feature("no such feature"),
        "an unknown name, through the device the render system holds"
    );
}

/// Threads bind, write and build pipeline layouts from buffers and
/// bind-group layouts that other threads destroy at the same moment. Each
/// call either succeeds (it cloned the resource out before it was removed) or
/// is refused `NotFound` — never another error, never a panic, never a wgpu
/// error.
#[test]
fn a_resource_destroyed_by_another_thread_is_found_or_not_found() {
    use khora_core::renderer::api::pipeline::PipelineLayoutDescriptor;
    use khora_core::renderer::ResourceError;

    let test = "a_resource_destroyed_by_another_thread_is_found_or_not_found";
    let Some(gpu) = gpu(test) else {
        return;
    };
    let device: Arc<dyn GraphicsDevice> = Arc::new(headless_device(&gpu));

    #[derive(Default)]
    struct Shared {
        buffers: Vec<BufferId>,
        layouts: Vec<BindGroupLayoutId>,
    }
    let shared = Arc::new(Mutex::new(Shared::default()));

    fn found_or_not(call: &str, result: Result<(), ResourceError>) -> Result<(), String> {
        match result {
            Ok(()) | Err(ResourceError::NotFound) => Ok(()),
            Err(other) => Err(failed(call, other)),
        }
    }

    fn uniform_layout() -> Vec<BindGroupLayoutEntry> {
        vec![BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStageFlags::FRAGMENT,
            ty: BindingType::Buffer {
                ty: BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
        }]
    }

    let mut workers: Vec<(String, Worker)> = Vec::new();
    for thread in 0..2 {
        let device = Arc::clone(&device);
        let shared = Arc::clone(&shared);
        workers.push((
            format!("makers-{thread}"),
            Box::new(move || {
                let start = Instant::now();
                while start.elapsed() < WORK {
                    let buffer = device
                        .create_buffer(&buffer_descriptor())
                        .map_err(|e| failed("create_buffer", e))?;
                    let entries = uniform_layout();
                    let layout = device
                        .create_bind_group_layout(&BindGroupLayoutDescriptor {
                            label: Some("shared layout"),
                            entries: &entries,
                        })
                        .map_err(|e| failed("create_bind_group_layout", e))?;
                    let mut shared = shared.lock().map_err(|_| "shared list poisoned")?;
                    shared.buffers.push(buffer);
                    shared.layouts.push(layout);
                }
                Ok(())
            }),
        ));
    }
    for thread in 0..3 {
        let device = Arc::clone(&device);
        let shared = Arc::clone(&shared);
        workers.push((
            format!("destroyers-{thread}"),
            Box::new(move || {
                let mut rng = XorShift::seeded(400 + thread);
                let start = Instant::now();
                while start.elapsed() < WORK {
                    let (buffer, layout) = {
                        let mut shared = shared.lock().map_err(|_| "shared list poisoned")?;
                        let buffer = (!shared.buffers.is_empty()).then(|| {
                            let i = rng.below(shared.buffers.len());
                            shared.buffers.swap_remove(i)
                        });
                        let layout = (!shared.layouts.is_empty()).then(|| {
                            let i = rng.below(shared.layouts.len());
                            shared.layouts.swap_remove(i)
                        });
                        (buffer, layout)
                    };
                    if let Some(buffer) = buffer {
                        device
                            .destroy_buffer(buffer)
                            .map_err(|e| failed("destroy_buffer", e))?;
                    }
                    if let Some(layout) = layout {
                        device
                            .destroy_bind_group_layout(layout)
                            .map_err(|e| failed("destroy_bind_group_layout", e))?;
                    }
                }
                Ok(())
            }),
        ));
    }
    for thread in 0..3 {
        let device = Arc::clone(&device);
        let shared = Arc::clone(&shared);
        workers.push((
            format!("users-{thread}"),
            Box::new(move || {
                let mut rng = XorShift::seeded(500 + thread);
                let start = Instant::now();
                while start.elapsed() < WORK {
                    let picked = {
                        let shared = shared.lock().map_err(|_| "shared list poisoned")?;
                        (!shared.buffers.is_empty() && !shared.layouts.is_empty()).then(|| {
                            (
                                shared.buffers[rng.below(shared.buffers.len())],
                                shared.layouts[rng.below(shared.layouts.len())],
                            )
                        })
                    };
                    let Some((buffer, layout)) = picked else {
                        continue;
                    };
                    found_or_not(
                        "write_buffer",
                        device.write_buffer(buffer, 0, &[rng.next() as u8; 16]),
                    )?;
                    let entries = [BindGroupEntry::buffer(0, buffer, 0, None)];
                    match device.create_bind_group(&BindGroupDescriptor {
                        label: Some("shared bind group"),
                        layout,
                        entries: &entries,
                    }) {
                        Ok(id) => device
                            .destroy_bind_group(id)
                            .map_err(|e| failed("destroy_bind_group", e))?,
                        Err(e) => found_or_not("create_bind_group", Err(e))?,
                    }
                    let layouts = [layout];
                    if let Err(e) = device.create_pipeline_layout(&PipelineLayoutDescriptor {
                        label: Some(Cow::Borrowed("shared pipeline layout")),
                        bind_group_layouts: &layouts,
                    }) {
                        found_or_not("create_pipeline_layout", Err(e))?;
                    }
                }
                Ok(())
            }),
        ));
    }
    run_watched("resources destroyed while others use them", workers);

    assert_eq!(
        gpu.reported_errors(),
        Vec::<String>::new(),
        "wgpu reported errors"
    );
}

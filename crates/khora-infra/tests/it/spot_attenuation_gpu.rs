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

//! `calculate_spot_attenuation` (`khora::lighting::attenuation`) evaluated on a
//! real adapter: the spot cone's falloff is a smoothstep from the outer cone to
//! the inner one, a hard edge when the two cones are equal, and a finite value
//! in `[0, 1]` for any cone, however small.
//!
//! The probe shader (`shaders/tests/spot_attenuation_probe.wgsl`) is composed
//! by the engine's own `WgpuPipelineSystem` — the same composer, lib modules
//! and shader defs every lit pipeline goes through — and the WGSL it hands the
//! device is caught by a recording `GraphicsDevice`, then built as a compute
//! pipeline on a real wgpu device (any adapter, the software fallback
//! included, as the light-culling tests get theirs). Each test feeds a
//! buffer of `(light_dir, spot_direction, inner_cos, outer_cos)` samples and
//! reads one falloff back per sample. Without any adapter each test returns
//! early.

use std::sync::mpsc;
use std::sync::{Mutex, OnceLock};

use khora_core::math::{Vec3, FRAC_PI_2, PI};
use khora_core::renderer::api::command::{
    BindGroupDescriptor, BindGroupId, BindGroupLayoutDescriptor, BindGroupLayoutId, CommandBufferId,
};
use khora_core::renderer::api::device::GraphicsAdapterInfo;
use khora_core::renderer::api::pipeline::{
    ComputePipelineDescriptor, ComputePipelineId, ComputePipelineSpec, PipelineLayoutDescriptor,
    PipelineLayoutId, RenderPipelineDescriptor, RenderPipelineId,
};
use khora_core::renderer::api::resource::{
    BufferDescriptor, BufferId, SamplerDescriptor, SamplerId, TextureDescriptor, TextureFormat,
    TextureId, TextureViewDescriptor, TextureViewId,
};
use khora_core::renderer::api::shader::{
    ShaderModuleDescriptor, ShaderModuleId, ShaderSourceData, ShaderVariantKey,
};
use khora_core::renderer::error::ResourceError;
use khora_core::renderer::traits::{CommandEncoder, GraphicsDevice, PipelineSystem};
use khora_infra::graphics::WgpuPipelineSystem;

/// The probe's logical name: the one the shader hot-reload watcher gives its
/// file, so an edit to it overlays the same module.
const PROBE: &str = "khora::tests::spot_attenuation_probe";
const PROBE_WGSL: &str =
    include_str!("../../src/graphics/shader/shaders/tests/spot_attenuation_probe.wgsl");
/// The probe's `@workgroup_size`.
const WORKGROUP: u32 = 64;
/// Bytes of one `SpotSample` in the probe's storage buffer (three `vec4<f32>`).
const SAMPLE_BYTES: u64 = 48;

// ─── Inputs, on the CPU ─────────────────────────────────────────────────────

/// One evaluation of `calculate_spot_attenuation`.
#[derive(Clone, Copy, Debug)]
struct SpotSample {
    /// From the shaded point towards the light.
    light_dir: Vec3,
    /// The spot's axis, unit length.
    spot_direction: Vec3,
    inner_cos: f32,
    outer_cos: f32,
}

impl SpotSample {
    /// A point `angle` radians off a spot's `axis`, in the plane of `axis` and
    /// `side` (unit, perpendicular to `axis`). On the Z axis with `side = X`
    /// the shader's `dot(-light_dir, spot_direction)` is exactly `cos(angle)`.
    fn off_axis(axis: Vec3, side: Vec3, angle: f32, inner_cos: f32, outer_cos: f32) -> Self {
        let towards_point = axis * angle.cos() + side * angle.sin();
        Self {
            light_dir: -towards_point,
            spot_direction: axis,
            inner_cos,
            outer_cos,
        }
    }

    /// The cosine the shader compares to the cones, computed the way it does.
    fn cos_angle(&self) -> f32 {
        (-self.light_dir).dot(self.spot_direction)
    }

    fn words(&self) -> [f32; 12] {
        let (l, s) = (self.light_dir, self.spot_direction);
        [
            l.x,
            l.y,
            l.z,
            0.0,
            s.x,
            s.y,
            s.z,
            0.0,
            self.inner_cos,
            self.outer_cos,
            0.0,
            0.0,
        ]
    }
}

/// A unit vector perpendicular to the unit `axis`.
fn side_of(axis: Vec3) -> Vec3 {
    let helper = if axis.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    axis.cross(helper).normalize()
}

/// The axes the spots point along besides `Z`: arbitrary, downwards, oblique.
fn oblique_axes() -> [Vec3; 3] {
    [
        Vec3::new(1.0, 2.0, 3.0).normalize(),
        Vec3::new(0.0, -1.0, 0.0),
        Vec3::new(-0.3, -1.0, 0.2).normalize(),
    ]
}

/// `smoothstep(edge0, edge1, x)` as WGSL defines it, for `edge0 < edge1`.
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// `[start, end]` in degrees, every `step` degrees, as radians.
fn degrees(start: f32, end: f32, step: f32) -> Vec<f32> {
    let count = ((end - start) / step).round() as u32;
    (0..=count)
        .map(|i| (start + i as f32 * step).to_radians())
        .collect()
}

// ─── The probe, on the GPU ──────────────────────────────────────────────────

/// A `GraphicsDevice` that keeps the WGSL the `PipelineSystem` composes and
/// builds nothing: the module is compiled on the real device afterwards.
#[derive(Debug, Default)]
struct ComposedWgsl {
    source: Mutex<Option<String>>,
}

impl GraphicsDevice for ComposedWgsl {
    fn create_shader_module(
        &self,
        d: &ShaderModuleDescriptor,
    ) -> Result<ShaderModuleId, ResourceError> {
        let ShaderSourceData::Wgsl(source) = &d.source;
        *self.source.lock().expect("the composed-source lock") = Some(source.to_string());
        Ok(ShaderModuleId(0))
    }
    fn destroy_shader_module(&self, _id: ShaderModuleId) -> Result<(), ResourceError> {
        unreachable!("the probe is composed, never destroyed")
    }
    fn create_render_pipeline(
        &self,
        _d: &RenderPipelineDescriptor,
    ) -> Result<RenderPipelineId, ResourceError> {
        unreachable!("the probe is a compute shader")
    }
    fn create_pipeline_layout(
        &self,
        _d: &PipelineLayoutDescriptor,
    ) -> Result<PipelineLayoutId, ResourceError> {
        Ok(PipelineLayoutId(0))
    }
    fn destroy_render_pipeline(&self, _id: RenderPipelineId) -> Result<(), ResourceError> {
        unreachable!()
    }
    fn create_compute_pipeline(
        &self,
        _d: &ComputePipelineDescriptor,
    ) -> Result<ComputePipelineId, ResourceError> {
        Ok(ComputePipelineId(0))
    }
    fn destroy_compute_pipeline(&self, _id: ComputePipelineId) -> Result<(), ResourceError> {
        unreachable!()
    }
    fn create_bind_group_layout(
        &self,
        _d: &BindGroupLayoutDescriptor,
    ) -> Result<BindGroupLayoutId, ResourceError> {
        unreachable!("the probe's spec names no layout")
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

/// The probe as the engine composes it: overlaid under its logical name and
/// built through `PipelineSystem::compute_pipeline`, which composes it against
/// the registered lib modules.
fn composed_probe() -> String {
    let system = WgpuPipelineSystem::new().expect("the pipeline system registers its libs");
    system.set_overlay_source(PROBE, PROBE_WGSL.to_owned());
    let capture = ComposedWgsl::default();
    system
        .compute_pipeline(
            &capture,
            &ComputePipelineSpec {
                label: "spot attenuation probe",
                shader: PROBE,
                variant: ShaderVariantKey::empty(),
                bind_group_layouts: Vec::new(),
                entry_point: "evaluate",
            },
        )
        .unwrap_or_else(|e| panic!("the spot attenuation probe does not compose: {e:?}"));
    let source = capture
        .source
        .lock()
        .expect("the composed-source lock")
        .take();
    source.expect("composing the probe hands its WGSL to the device")
}

struct Probe {
    device: wgpu::Device,
    queue: wgpu::Queue,
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,
}

/// One device + probe pipeline shared by every test, or `None` when the
/// machine has no adapter at all.
fn probe() -> Option<&'static Probe> {
    static PROBE_PIPELINE: OnceLock<Option<Probe>> = OnceLock::new();
    PROBE_PIPELINE.get_or_init(create_probe).as_ref()
}

fn create_probe() -> Option<Probe> {
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
        log::warn!("spot_attenuation_gpu: no wgpu adapter, GPU spot falloff tests skipped");
        return None;
    };
    let (device, queue) =
        match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("spot attenuation test device"),
            required_features: wgpu::Features::empty(),
            required_limits: adapter.limits(),
            experimental_features: wgpu::ExperimentalFeatures::default(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        })) {
            Ok(pair) => pair,
            Err(e) => {
                log::warn!("spot_attenuation_gpu: no device ({e}), GPU spot falloff tests skipped");
                return None;
            }
        };
    let wgsl = composed_probe();

    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(PROBE),
        source: wgpu::ShaderSource::Wgsl(wgsl.into()),
    });
    let entry = |binding: u32, read_only: bool| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("spot attenuation probe layout"),
        entries: &[entry(0, true), entry(1, false)],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("spot attenuation probe pipeline layout"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("spot attenuation probe pipeline"),
        layout: Some(&pipeline_layout),
        module: &module,
        entry_point: Some("evaluate"),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
    });
    if let Some(error) = pollster::block_on(scope.pop()) {
        panic!("the composed spot attenuation probe does not build a compute pipeline: {error}");
    }
    Some(Probe {
        device,
        queue,
        layout,
        pipeline,
    })
}

/// One `calculate_spot_attenuation` value per sample, read back from the GPU;
/// `None` without an adapter.
fn falloffs(samples: &[SpotSample]) -> Option<Vec<f32>> {
    assert!(!samples.is_empty(), "a probe dispatch needs samples");
    let _turn = crate::gpu_test::exclusive_gpu();
    let probe = probe()?;
    let device = &probe.device;

    let words: Vec<f32> = samples.iter().flat_map(SpotSample::words).collect();
    let input_size = samples.len() as u64 * SAMPLE_BYTES;
    let output_size = samples.len() as u64 * std::mem::size_of::<f32>() as u64;

    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let buffer = |label: &str, size: u64, usage: wgpu::BufferUsages| {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage,
            mapped_at_creation: false,
        })
    };
    let input = buffer(
        "spot samples",
        input_size,
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    );
    let output = buffer(
        "spot falloffs",
        output_size,
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
    );
    let readback = buffer(
        "spot falloffs readback",
        output_size,
        wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
    );
    probe
        .queue
        .write_buffer(&input, 0, bytemuck::cast_slice(&words));

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("spot attenuation probe bind group"),
        layout: &probe.layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: input.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: output.as_entire_binding(),
            },
        ],
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("spot attenuation probe encoder"),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("spot attenuation probe pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&probe.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups((samples.len() as u32).div_ceil(WORKGROUP), 1, 1);
    }
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, output_size);
    probe.queue.submit([encoder.finish()]);
    if let Some(error) = pollster::block_on(scope.pop()) {
        panic!("the spot attenuation probe dispatch fails validation: {error}");
    }

    let (sender, receiver) = mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("the device finishes the probe dispatch");
    receiver
        .recv()
        .expect("the map callback runs")
        .expect("the falloff readback maps");
    let values = readback
        .slice(..)
        .get_mapped_range()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect();
    readback.unmap();
    Some(values)
}

/// Fails with every `(sample, value, why)` a check refused, at most a screenful.
fn assert_none_refused(what: &str, refused: &[(SpotSample, f32, String)]) {
    if refused.is_empty() {
        return;
    }
    let shown: Vec<String> = refused
        .iter()
        .take(12)
        .map(|(s, v, why)| {
            format!(
                "  cos_angle {:.9}, inner_cos {:.9}, outer_cos {:.9}: got {v} — {why}",
                s.cos_angle(),
                s.inner_cos,
                s.outer_cos
            )
        })
        .collect();
    panic!(
        "{what}: {} of the samples are wrong\n{}",
        refused.len(),
        shown.join("\n")
    );
}

// ─── Behaviour ──────────────────────────────────────────────────────────────

/// **For a proper cone, the falloff is the smoothstep it has always been** —
/// guarding the cone must not change how a usable spot lights. A proper cone
/// is one whose inner and outer cosines are at least `1e-4` apart, which every
/// cone below is by far.
#[test]
fn spot_falloff_matches_smoothstep_where_it_is_defined() {
    // (inner, outer) half-angles in degrees.
    let all_axes_cones: &[(f32, f32)] = &[(0.0, 45.0), (10.0, 20.0), (40.0, 60.0), (5.0, 89.0)];
    let z_only_cones: &[(f32, f32)] = &[(20.0, 30.0), (29.0, 30.0), (60.0, 85.0)];

    let mut samples = Vec::new();
    let mut add_cone = |axis: Vec3, (inner, outer): (f32, f32)| {
        let (inner_cos, outer_cos) = (inner.to_radians().cos(), outer.to_radians().cos());
        assert!(
            inner_cos - outer_cos >= 1e-4,
            "cone ({inner}°, {outer}°) is not a proper cone"
        );
        let side = side_of(axis);
        for angle in degrees(0.0, (outer + 10.0).min(180.0), 0.25) {
            samples.push(SpotSample::off_axis(
                axis, side, angle, inner_cos, outer_cos,
            ));
        }
    };
    for &cone in all_axes_cones.iter().chain(z_only_cones) {
        add_cone(Vec3::Z, cone);
    }
    for axis in oblique_axes() {
        for &cone in all_axes_cones {
            add_cone(axis, cone);
        }
    }

    let Some(values) = falloffs(&samples) else {
        return;
    };
    let refused: Vec<_> = samples
        .iter()
        .zip(&values)
        .filter_map(|(s, &v)| {
            let expected = smoothstep(s.outer_cos, s.inner_cos, s.cos_angle());
            (!v.is_finite() || (v - expected).abs() > 1e-5)
                .then(|| (*s, v, format!("smoothstep gives {expected}")))
        })
        .collect();
    assert_none_refused("the spot falloff departs from smoothstep", &refused);
}

/// **A spot whose inner and outer cones are equal has a hard edge**: fully lit
/// inside the cone, dark outside, and never NaN — not even exactly on the
/// edge, where a plain `smoothstep(edge, edge, x)` divides zero by zero.
///
/// "Inside" is at least half a degree inside the edge; exactly on it, and a
/// thousandth of a degree inside it, the value only has to be a finite one in
/// `[0, 1]`. Anything outside the edge is `0`.
#[test]
fn a_spot_with_equal_cones_has_a_hard_edge() {
    const EDGES: [f32; 6] = [5.0, 15.0, 30.0, 45.0, 60.0, 89.0];
    const INSIDE_BY: [f32; 3] = [0.5, 1.0, 5.0];
    const OUTSIDE_BY: [f32; 4] = [0.001, 0.5, 1.0, 5.0];
    const NEAR_INSIDE_BY: f32 = 0.001;

    #[derive(Clone, Copy, PartialEq)]
    enum Expect {
        Lit,
        Dark,
        Bounded,
    }

    let mut cases: Vec<(SpotSample, Expect)> = Vec::new();
    for edge in EDGES {
        let edge_cos = edge.to_radians().cos();
        let mut at = |axis: Vec3, degrees: f32, expect: Expect| {
            let sample = SpotSample::off_axis(
                axis,
                side_of(axis),
                degrees.to_radians(),
                edge_cos,
                edge_cos,
            );
            cases.push((sample, expect));
        };
        // On the Z axis the shader's cosine is exactly `cos(angle)`, so the
        // edge sample sits exactly on the edge.
        at(Vec3::Z, edge, Expect::Bounded);
        at(Vec3::Z, edge - NEAR_INSIDE_BY, Expect::Bounded);
        at(Vec3::Z, 0.0, Expect::Lit);
        for by in INSIDE_BY.into_iter().filter(|by| *by <= edge) {
            at(Vec3::Z, edge - by, Expect::Lit);
        }
        for by in OUTSIDE_BY {
            at(Vec3::Z, edge + by, Expect::Dark);
        }
        for axis in oblique_axes() {
            at(axis, edge, Expect::Bounded);
            at(axis, 0.0, Expect::Lit);
            at(axis, edge - 1.0, Expect::Lit);
            at(axis, edge + 1.0, Expect::Dark);
        }
    }

    let samples: Vec<SpotSample> = cases.iter().map(|(s, _)| *s).collect();
    let Some(values) = falloffs(&samples) else {
        return;
    };
    let refused: Vec<_> = cases
        .iter()
        .zip(&values)
        .filter_map(|(&(s, expect), &v)| {
            let why = if !v.is_finite() {
                Some("not finite")
            } else if !(0.0..=1.0).contains(&v) {
                Some("outside [0, 1]")
            } else if expect == Expect::Lit && (v - 1.0).abs() > 1e-6 {
                Some("inside the cone, not fully lit")
            } else if expect == Expect::Dark && v.abs() > 1e-6 {
                Some("outside the cone, not dark")
            } else {
                None
            };
            why.map(|why| (s, v, why.to_owned()))
        })
        .collect();
    assert_none_refused("an equal-cone spot has no hard edge", &refused);
}

/// **A cone too small for its cosines still gives a finite falloff**: at
/// half-angles of `1e-4` rad both cosines round to `1.0` in `f32`, so the
/// cones are equal even when authored apart, and the point on the axis sits
/// exactly on the edge. Every point, on the axis or off it, gets a finite value
/// in `[0, 1]`.
#[test]
fn a_cone_too_small_for_its_cosines_stays_finite() {
    // (inner, outer) half-angles in radians.
    const CONES: [(f32, f32); 3] = [(0.5e-4, 1e-4), (1e-4, 1e-4), (0.0, 1e-4)];
    const ANGLES: [f32; 9] = [0.0, 0.5e-4, 1e-4, 2e-4, 1e-3, 0.1, 1.0, FRAC_PI_2, PI];

    let mut samples = Vec::new();
    for (inner, outer) in CONES {
        let (inner_cos, outer_cos) = (inner.cos(), outer.cos());
        assert!(
            inner_cos == 1.0 && outer_cos == 1.0,
            "the premise: cones of {inner} and {outer} rad have cosines {inner_cos} and {outer_cos}, not both 1"
        );
        let axes = std::iter::once(Vec3::Z).chain(oblique_axes());
        for axis in axes {
            let side = side_of(axis);
            for angle in ANGLES {
                samples.push(SpotSample::off_axis(
                    axis, side, angle, inner_cos, outer_cos,
                ));
            }
            // The point straight down the axis, through a normalised vector:
            // its cosine may land a rounding step above or below 1.
            samples.push(SpotSample {
                light_dir: -axis,
                spot_direction: axis,
                inner_cos,
                outer_cos,
            });
        }
    }

    let Some(values) = falloffs(&samples) else {
        return;
    };
    let refused: Vec<_> = samples
        .iter()
        .zip(&values)
        .filter_map(|(s, &v)| {
            let why = if !v.is_finite() {
                Some("not finite")
            } else if !(0.0..=1.0).contains(&v) {
                Some("outside [0, 1]")
            } else {
                None
            };
            why.map(|why| (*s, v, why.to_owned()))
        })
        .collect();
    assert_none_refused(
        "a tiny cone's falloff is not a finite value in [0, 1]",
        &refused,
    );
}

/// **A narrow cone keeps its smooth fade**: inner and outer half a tenth of a
/// degree apart (their cosines ~3e-5 apart) fade exactly as `smoothstep`
/// does — only cones with no span at all get a hard edge, so guarding the
/// equal case changes no light an author can tell from today's.
#[test]
fn a_narrow_cone_keeps_its_smooth_fade() {
    let (inner, outer) = (1.0_f32, 1.1_f32);
    let (inner_cos, outer_cos) = (inner.to_radians().cos(), outer.to_radians().cos());
    assert!(
        inner_cos > outer_cos,
        "a cone with a span between its edges"
    );
    let side = side_of(Vec3::Z);
    let samples: Vec<_> = degrees(0.9, 1.2, 0.005)
        .into_iter()
        .map(|angle| SpotSample::off_axis(Vec3::Z, side, angle, inner_cos, outer_cos))
        .collect();

    let Some(values) = falloffs(&samples) else {
        return;
    };
    let refused: Vec<_> = samples
        .iter()
        .zip(&values)
        .filter_map(|(s, &v)| {
            let expected = smoothstep(s.outer_cos, s.inner_cos, s.cos_angle());
            (!v.is_finite() || (v - expected).abs() > 1e-4)
                .then(|| (*s, v, format!("smoothstep gives {expected}")))
        })
        .collect();
    assert_none_refused("a narrow cone departs from smoothstep", &refused);
}

// ─── Edges of the inputs ────────────────────────────────────────────────────

/// A point whose cosine to a spot pointing down `Z` is exactly `cos` — the
/// shader's `dot(-light_dir, Z)` reads `cos` back unrounded. `|cos| > 1`
/// stands for a direction a rounding step longer than unit.
fn at_cos(cos: f32, inner_cos: f32, outer_cos: f32) -> SpotSample {
    let sin = (1.0 - cos * cos).max(0.0).sqrt();
    SpotSample {
        light_dir: Vec3::new(-sin, 0.0, -cos),
        spot_direction: Vec3::Z,
        inner_cos,
        outer_cos,
    }
}

/// The float `steps` representable values above `x` (below for negative
/// `steps`), for `x > 0`.
fn ulps_from(x: f32, steps: i32) -> f32 {
    f32::from_bits((x.to_bits() as i64 + steps as i64) as u32)
}

/// Cosines around a cone's two edges: on each, and a few rounding steps
/// either side.
fn around_the_edges(inner_cos: f32, outer_cos: f32) -> Vec<f32> {
    let mut cosines = Vec::new();
    for edge in [inner_cos, outer_cos] {
        cosines.push(edge);
        if edge > 0.0 {
            for steps in [-3, -1, 1, 3] {
                cosines.push(ulps_from(edge, steps));
            }
        }
    }
    cosines
}

/// **A span between the cones too small to be a normal float still gives a
/// finite falloff**: a value in `[0, 1]`, dark below the outer edge, lit from
/// the inner edge on — whether the device keeps the subnormal span or flushes
/// it to zero. (A device that flushes also flushes a subnormal cosine, or a
/// subnormal outer edge, so a cosine lands on the edge it sat beside: "dark"
/// and "lit" are asked only of normal cosines. An outer cosine this close to
/// 0 is a cone of about 90°, which `Light::check` refuses — this guards the
/// shader, not a light that shines.)
#[test]
fn a_subnormal_span_between_the_cones_stays_finite() {
    // (inner_cos, outer_cos): the span is subnormal, or the cosines are.
    let cones: [(f32, f32); 5] = [
        (1e-40, 0.0),
        (f32::from_bits(1), 0.0),
        (1.5e-38, 1.2e-38),
        (f32::MIN_POSITIVE, ulps_from(f32::MIN_POSITIVE, -1)),
        (ulps_from(f32::MIN_POSITIVE, 1), f32::MIN_POSITIVE),
    ];
    let mut cases = Vec::new();
    for (inner_cos, outer_cos) in cones {
        assert!(inner_cos > outer_cos, "the premise: a span, however small");
        assert!(
            (inner_cos - outer_cos) < f32::MIN_POSITIVE,
            "the premise: a subnormal span"
        );
        let mut cosines = around_the_edges(inner_cos, outer_cos);
        cosines.extend([0.0, -1e-45, -1e-30, 1e-30, 0.5, 1.0, -1.0]);
        for cos in cosines {
            cases.push(at_cos(cos, inner_cos, outer_cos));
        }
    }
    let Some(values) = falloffs(&cases) else {
        return;
    };
    let refused: Vec<_> = cases
        .iter()
        .zip(&values)
        .filter_map(|(s, &v)| {
            let cos = s.cos_angle();
            let why = if !v.is_finite() {
                Some("not finite")
            } else if !(0.0..=1.0).contains(&v) {
                Some("outside [0, 1]")
            } else if cos < s.outer_cos && cos.abs() >= f32::MIN_POSITIVE && v != 0.0 {
                Some("below the outer edge, not dark")
            } else if cos >= s.inner_cos && cos >= f32::MIN_POSITIVE && v != 1.0 {
                Some("at or inside the inner edge, not fully lit")
            } else {
                None
            };
            why.map(|why| (*s, v, why.to_owned()))
        })
        .collect();
    assert_none_refused("a subnormal span gives a broken falloff", &refused);
}

/// **An inner cone wider than the outer one still gives a finite falloff** —
/// `Light::check` reports such a spot, so it never shines, but the shader
/// must not hand a NaN to whoever calls it anyway.
#[test]
fn an_inner_cone_wider_than_the_outer_stays_finite() {
    let cones: [(f32, f32); 3] = [(0.5, 0.9), (-1.0, 1.0), (0.0, 1e-7)];
    let mut cases = Vec::new();
    for (inner_cos, outer_cos) in cones {
        let mut cosines = around_the_edges(inner_cos.max(1e-9), outer_cos);
        cosines.extend([-1.0, -0.5, 0.0, 0.25, 0.7, 0.95, 1.0]);
        for cos in cosines {
            cases.push(at_cos(cos, inner_cos, outer_cos));
        }
    }
    let Some(values) = falloffs(&cases) else {
        return;
    };
    let refused: Vec<_> = cases
        .iter()
        .zip(&values)
        .filter(|&(_, &v)| !v.is_finite() || !(0.0..=1.0).contains(&v))
        .map(|(s, &v)| (*s, v, "not a finite value in [0, 1]".to_owned()))
        .collect();
    assert_none_refused("an inverted cone's falloff", &refused);
}

/// **Straight down the axis a spot is fully lit, straight behind it dark** —
/// and a cosine a rounding step past one (directions normalised per fragment
/// are not exactly unit) changes neither.
#[test]
fn a_cosine_of_plus_or_minus_one_is_fully_lit_or_dark() {
    let cones: [(f32, f32); 3] = [(1.0, 0.5), (0.9, 0.8), (0.5, 0.5)];
    let mut cases = Vec::new();
    for (inner_cos, outer_cos) in cones {
        for cos in [1.0, ulps_from(1.0, 1), ulps_from(1.0, 4)] {
            cases.push((at_cos(cos, inner_cos, outer_cos), 1.0));
        }
        for cos in [-1.0, -ulps_from(1.0, 1), -ulps_from(1.0, 4)] {
            cases.push((at_cos(cos, inner_cos, outer_cos), 0.0));
        }
    }
    let samples: Vec<SpotSample> = cases.iter().map(|(s, _)| *s).collect();
    let Some(values) = falloffs(&samples) else {
        return;
    };
    let refused: Vec<_> = cases
        .iter()
        .zip(&values)
        .filter(|&(&(_, expected), &v)| v != expected)
        .map(|(&(s, expected), &v)| (s, v, format!("expected {expected}")))
        .collect();
    assert_none_refused("the falloff at the axis or behind it", &refused);
}

/// **A spot too thin for its cosines still lights its own axis**: below about
/// `3e-4` rad both cone cosines round to `1`, and so does the cosine of a
/// point straight down the axis — at angle `0`, inside any cone `Light::check`
/// accepts. That point is fully lit, as the inner edge of every cone with a
/// span is.
#[test]
fn a_spot_too_thin_for_its_cosines_lights_its_own_axis() {
    const CONES: [(f32, f32); 3] = [(0.0, 1e-4), (1e-4, 1e-4), (0.5e-4, 2e-4)];
    let samples: Vec<SpotSample> = CONES
        .iter()
        .map(|&(inner, outer)| {
            let (inner_cos, outer_cos) = (inner.cos(), outer.cos());
            assert!(
                inner_cos == 1.0 && outer_cos == 1.0,
                "the premise: both cosines round to 1"
            );
            at_cos(1.0, inner_cos, outer_cos)
        })
        .collect();
    let Some(values) = falloffs(&samples) else {
        return;
    };
    let refused: Vec<_> = samples
        .iter()
        .zip(&values)
        .filter(|(_, &v)| v != 1.0)
        .map(|(s, &v)| (*s, v, "the axis of the cone, not fully lit".to_owned()))
        .collect();
    assert_none_refused("a thin spot's axis is dark", &refused);
}

/// **The falloff never grows as the angle off the axis widens**, for proper,
/// narrow and equal cones alike — a spot never brightens towards its edge.
#[test]
fn the_falloff_never_grows_as_the_angle_widens() {
    let degree_cones: [(f32, f32); 5] = [
        (10.0, 20.0),
        (29.0, 30.0),
        (1.0, 1.001),
        (30.0, 30.0),
        (0.0, 89.0),
    ];
    let mut runs: Vec<Vec<SpotSample>> = Vec::new();
    for (inner, outer) in degree_cones {
        let (inner_cos, outer_cos) = (inner.to_radians().cos(), outer.to_radians().cos());
        let mut cosines: Vec<f32> = (0..=2000).map(|i| 1.0 - i as f32 / 1000.0).collect();
        cosines.extend(around_the_edges(inner_cos, outer_cos));
        // Every representable cosine across the fade of the narrow cones.
        if inner_cos - outer_cos < 1e-4 {
            let mut c = ulps_from(outer_cos, -4);
            while c <= ulps_from(inner_cos, 4) {
                cosines.push(c);
                c = ulps_from(c, 1);
            }
        }
        // Widening angle = falling cosine.
        cosines.sort_by(|a, b| b.total_cmp(a));
        cosines.dedup();
        runs.push(
            cosines
                .into_iter()
                .map(|cos| at_cos(cos, inner_cos, outer_cos))
                .collect(),
        );
    }
    let samples: Vec<SpotSample> = runs.iter().flatten().copied().collect();
    let Some(values) = falloffs(&samples) else {
        return;
    };
    let mut refused = Vec::new();
    let mut at = 0;
    for run in &runs {
        let run_values = &values[at..at + run.len()];
        for i in 1..run.len() {
            if run_values[i] > run_values[i - 1] {
                refused.push((
                    run[i],
                    run_values[i],
                    format!(
                        "brighter than {} at the narrower cosine {:.9}",
                        run_values[i - 1],
                        run[i - 1].cos_angle()
                    ),
                ));
            }
        }
        at += run.len();
    }
    assert_none_refused("the falloff grows with the angle", &refused);
}

/// **Any span from a millionth up keeps today's fade to within a millionth**:
/// on the `Z` axis, where the cosine is exact, the written-out falloff and
/// `smoothstep` part by no more than `1e-6`, however narrow the cone.
#[test]
fn the_fade_matches_smoothstep_to_a_millionth_for_any_span() {
    const SPANS: [f32; 7] = [1e-6, 3e-6, 1e-5, 1e-4, 1e-3, 1e-1, 1.0];
    const OUTERS: [f32; 4] = [0.0, 0.3, 0.5, 0.9];
    let mut samples = Vec::new();
    for outer_cos in OUTERS {
        for span in SPANS {
            let inner_cos = outer_cos + span;
            if inner_cos > 1.0 {
                continue;
            }
            let span = inner_cos - outer_cos;
            for i in 0..=200 {
                let cos = outer_cos - 0.25 * span + 1.5 * span * i as f32 / 200.0;
                samples.push(at_cos(cos, inner_cos, outer_cos));
            }
            for cos in around_the_edges(inner_cos, outer_cos.max(1e-9)) {
                samples.push(at_cos(cos, inner_cos, outer_cos));
            }
        }
    }
    let Some(values) = falloffs(&samples) else {
        return;
    };
    let refused: Vec<_> = samples
        .iter()
        .zip(&values)
        .filter_map(|(s, &v)| {
            let expected = smoothstep(s.outer_cos, s.inner_cos, s.cos_angle());
            (!v.is_finite() || (v - expected).abs() > 1e-6)
                .then(|| (*s, v, format!("smoothstep gives {expected}")))
        })
        .collect();
    assert_none_refused("the fade departs from smoothstep", &refused);
}

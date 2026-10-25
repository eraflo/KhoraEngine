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

//! `light_culling.wgsl` dispatched on a real adapter: each light lands in the
//! screen tiles its sphere covers, in the framebuffer convention the Forward+
//! fragment shader reads them back with (row 0 at the top).
//!
//! The shader is bound exactly as the Forward+ lane binds it (uniforms, lights,
//! index list, grid), fed a `CullingUniformsData` and `GpuLight`s, and the grid
//! and index list are read back. Every expectation is the tile the light's
//! centre projects to on the CPU through the same view-projection, widened by
//! the light's radius in pixels. Without any adapter (a CI runner without a
//! GPU or a software fallback) each test returns early.

use std::collections::BTreeSet;
use std::sync::mpsc;
use std::sync::OnceLock;

use khora_core::math::{Mat4, Vec3, Vec4, FRAC_PI_3};
use khora_core::renderer::light::CullingUniformsData;
use khora_core::renderer::GpuLight;

const LIGHT_CULLING_WGSL: &str =
    include_str!("../../src/graphics/shader/shaders/pipelines/light_culling.wgsl");

/// Slots per tile in the index list: tile `t`'s lights start at
/// `t * SHADER_LIGHTS_PER_TILE` (the shader's `MAX_LIGHTS_PER_TILE`).
const SHADER_LIGHTS_PER_TILE: u32 = 128;
/// The lane's culling uniform buffer is 256 bytes, bound whole.
const UNIFORM_BUFFER_SIZE: u64 = 256;
/// The lane's light buffer is 64 KiB, bound whole.
const LIGHT_BUFFER_SIZE: u64 = 64 * 1024;

/// Colour target: 20 x 12 tiles of 16 px, 10 x 6 tiles of 32 px.
const WIDTH: f32 = 320.0;
const HEIGHT: f32 = 192.0;
const FOV_Y: f32 = FRAC_PI_3;
const NEAR: f32 = 0.1;
const FAR: f32 = 100.0;

// ─── Camera, on the CPU ─────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum Lens {
    /// Pixels per world unit at view depth 1.
    Perspective { focal_px: f32 },
    /// Pixels per world unit at any depth.
    Orthographic { px_per_unit: f32 },
}

#[derive(Clone, Copy)]
struct Camera {
    view: Mat4,
    view_proj: Mat4,
    lens: Lens,
}

impl Camera {
    fn perspective(eye: Vec3, target: Vec3) -> Self {
        let view = Mat4::look_at_rh(eye, target, Vec3::Y).expect("the camera looks somewhere");
        let proj = Mat4::perspective_rh_zo(FOV_Y, WIDTH / HEIGHT, NEAR, FAR);
        Self {
            view,
            view_proj: proj * view,
            lens: Lens::Perspective {
                focal_px: HEIGHT * 0.5 / (FOV_Y * 0.5).tan(),
            },
        }
    }

    fn orthographic(eye: Vec3, target: Vec3, half_height: f32) -> Self {
        let half_width = half_height * WIDTH / HEIGHT;
        let view = Mat4::look_at_rh(eye, target, Vec3::Y).expect("the camera looks somewhere");
        let proj = Mat4::orthographic_rh_zo(
            -half_width,
            half_width,
            -half_height,
            half_height,
            NEAR,
            FAR,
        );
        Self {
            view,
            view_proj: proj * view,
            lens: Lens::Orthographic {
                px_per_unit: HEIGHT / (2.0 * half_height),
            },
        }
    }

    /// The framebuffer pixel `p` projects to (y down), or `None` behind the eye.
    fn pixel_of(&self, p: Vec3) -> Option<[f32; 2]> {
        let clip = self.view_proj * Vec4::new(p.x, p.y, p.z, 1.0);
        if clip.w <= 0.0 {
            return None;
        }
        let ndc = [clip.x / clip.w, clip.y / clip.w];
        Some([(ndc[0] + 1.0) * 0.5 * WIDTH, (1.0 - ndc[1]) * 0.5 * HEIGHT])
    }

    /// Distance of `p` in front of the eye, along the view direction.
    fn depth_of(&self, p: Vec3) -> f32 {
        -self.view.transform_point(p).z
    }

    /// The world point seen at `pixel`, `depth` in front of the eye.
    fn at_pixel(&self, pixel: [f32; 2], depth: f32) -> Vec3 {
        let inverse = self
            .view_proj
            .inverse()
            .expect("the view-projection is invertible");
        let ndc = [2.0 * pixel[0] / WIDTH - 1.0, 1.0 - 2.0 * pixel[1] / HEIGHT];
        let unproject = |z: f32| {
            let c = inverse * Vec4::new(ndc[0], ndc[1], z, 1.0);
            Vec3::new(c.x / c.w, c.y / c.w, c.z / c.w)
        };
        let (near, far) = (unproject(0.0), unproject(1.0));
        let (near_depth, far_depth) = (self.depth_of(near), self.depth_of(far));
        let point = near + (far - near) * ((depth - near_depth) / (far_depth - near_depth));

        let back = self
            .pixel_of(point)
            .expect("the point is in front of the eye");
        assert!(
            (back[0] - pixel[0]).abs() < 0.1 && (back[1] - pixel[1]).abs() < 0.1,
            "CPU unprojection of {pixel:?} reprojects to {back:?}"
        );
        assert!((self.depth_of(point) - depth).abs() < 1e-2 * depth.max(1.0));
        point
    }

    /// The world radius a sphere at `depth` needs to cover `pixels` on screen.
    fn radius_for_pixels(&self, pixels: f32, depth: f32) -> f32 {
        match self.lens {
            Lens::Perspective { focal_px } => pixels * depth / focal_px,
            Lens::Orthographic { px_per_unit } => pixels / px_per_unit,
        }
    }

    /// The camera's view direction, unit length.
    fn forward(eye: Vec3, target: Vec3) -> Vec3 {
        (target - eye).normalize()
    }
}

/// The centre pixel of tile `(tx, ty)` at `tile_size`.
fn tile_centre(tx: u32, ty: u32, tile_size: u32) -> [f32; 2] {
    let ts = tile_size as f32;
    [(tx as f32 + 0.5) * ts, (ty as f32 + 0.5) * ts]
}

fn tile_counts(tile_size: u32) -> (u32, u32) {
    (
        (WIDTH as u32).div_ceil(tile_size),
        (HEIGHT as u32).div_ceil(tile_size),
    )
}

/// The tiles a sphere projecting to a disc of `radius_px` around `pixel` is
/// culled into by side planes: those whose column and row both come within
/// the radius (corner tiles included — a plane test is conservative there).
///
/// Panics when a tile sits within a pixel of the radius: the test geometry
/// would then not say which way the tile falls.
fn tiles_under_disc(pixel: [f32; 2], radius_px: f32, tile_size: u32) -> BTreeSet<(u32, u32)> {
    let (tiles_x, tiles_y) = tile_counts(tile_size);
    let ts = tile_size as f32;
    let gap = |p: f32, cell: u32| {
        let (lo, hi) = (cell as f32 * ts, (cell + 1) as f32 * ts);
        (lo - p).max(p - hi).max(0.0)
    };
    let clear = |g: f32| g == 0.0 || (g - radius_px).abs() > 1.0;
    let mut tiles = BTreeSet::new();
    for ty in 0..tiles_y {
        for tx in 0..tiles_x {
            let (gx, gy) = (gap(pixel[0], tx), gap(pixel[1], ty));
            assert!(
                clear(gx) && clear(gy),
                "ambiguous test geometry: tile ({tx}, {ty}) is {gx}/{gy} px from {pixel:?}, radius {radius_px} px"
            );
            if gx < radius_px && gy < radius_px {
                tiles.insert((tx, ty));
            }
        }
    }
    tiles
}

fn point_light(position: Vec3, range: f32) -> GpuLight {
    GpuLight {
        position: [position.x, position.y, position.z],
        range,
        light_type: GpuLight::TYPE_POINT,
        ..GpuLight::default()
    }
}

fn directional_light() -> GpuLight {
    GpuLight {
        position: [0.0; 3],
        range: 0.0,
        direction: [0.0, -1.0, 0.0],
        light_type: GpuLight::TYPE_DIRECTIONAL,
        ..GpuLight::default()
    }
}

// ─── The shader, on the GPU ─────────────────────────────────────────────────

pub(crate) struct Culler {
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    pub(crate) layout: wgpu::BindGroupLayout,
    pub(crate) pipeline: wgpu::ComputePipeline,
}

/// One device + culling pipeline shared by every test, or `None` when the
/// machine has no adapter at all.
pub(crate) fn culler() -> Option<&'static Culler> {
    static CULLER: OnceLock<Option<Culler>> = OnceLock::new();
    CULLER.get_or_init(create_culler).as_ref()
}

fn create_culler() -> Option<Culler> {
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
        log::warn!("light_culling_gpu: no wgpu adapter, GPU culling tests skipped");
        return None;
    };
    let (device, queue) =
        match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("light culling test device"),
            required_features: wgpu::Features::empty(),
            required_limits: adapter.limits(),
            experimental_features: wgpu::ExperimentalFeatures::default(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        })) {
            Ok(pair) => pair,
            Err(e) => {
                log::warn!("light_culling_gpu: no device ({e}), GPU culling tests skipped");
                return None;
            }
        };

    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("light_culling.wgsl"),
        source: wgpu::ShaderSource::Wgsl(LIGHT_CULLING_WGSL.into()),
    });
    // As `fp_culling_layout_entries` in `khora-lanes` `forward_plus/gpu.rs`.
    let entry = |binding: u32, ty: wgpu::BufferBindingType| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("light culling layout"),
        entries: &[
            entry(0, wgpu::BufferBindingType::Uniform),
            entry(1, wgpu::BufferBindingType::Storage { read_only: true }),
            entry(2, wgpu::BufferBindingType::Storage { read_only: false }),
            entry(3, wgpu::BufferBindingType::Storage { read_only: false }),
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("light culling pipeline layout"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("light culling pipeline"),
        layout: Some(&pipeline_layout),
        module: &module,
        entry_point: Some("cs_main"),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
    });
    if let Some(error) = pollster::block_on(scope.pop()) {
        panic!("light_culling.wgsl does not build a compute pipeline: {error}");
    }

    Some(Culler {
        device,
        queue,
        layout,
        pipeline,
    })
}

/// What one culling dispatch is asked to do.
struct Frame {
    camera: Camera,
    tile_size: u32,
    max_lights_per_tile: u32,
}

impl Frame {
    fn new(camera: Camera, tile_size: u32) -> Self {
        Self {
            camera,
            tile_size,
            max_lights_per_tile: SHADER_LIGHTS_PER_TILE,
        }
    }
}

/// Per tile, the light indices the shader listed, read back from the grid
/// (`(offset, count)` per tile) and the index list.
struct TileLists {
    tiles_x: u32,
    tiles_y: u32,
    lists: Vec<Vec<u32>>,
}

impl TileLists {
    fn at(&self, tx: u32, ty: u32) -> &[u32] {
        &self.lists[(ty * self.tiles_x + tx) as usize]
    }

    /// Every tile that lists `light`.
    fn tiles_of(&self, light: u32) -> BTreeSet<(u32, u32)> {
        let mut tiles = BTreeSet::new();
        for ty in 0..self.tiles_y {
            for tx in 0..self.tiles_x {
                if self.at(tx, ty).contains(&light) {
                    tiles.insert((tx, ty));
                }
            }
        }
        tiles
    }
}

/// Runs `light_culling.wgsl` over `lights` for `frame`; `None` without a GPU.
fn cull(frame: &Frame, lights: &[GpuLight]) -> Option<TileLists> {
    let culler = culler()?;
    let device = &culler.device;
    let (tiles_x, tiles_y) = tile_counts(frame.tile_size);
    let tiles = u64::from(tiles_x * tiles_y);

    let uniforms = CullingUniformsData {
        view_projection: frame.camera.view_proj.to_cols_array_2d(),
        screen_dimensions: [WIDTH, HEIGHT],
        tile_count: [tiles_x, tiles_y],
        num_lights: lights.len() as u32,
        tile_size: frame.tile_size,
        max_lights_per_tile: frame.max_lights_per_tile,
        _padding: 0,
    };
    let light_bytes: &[u8] = bytemuck::cast_slice(lights);
    assert!(light_bytes.len() as u64 <= LIGHT_BUFFER_SIZE);

    // Sized as the lane sizes them (`ensure_tile_buffers`).
    let u32_size = std::mem::size_of::<u32>() as u64;
    let index_size =
        tiles * u64::from(frame.max_lights_per_tile.max(SHADER_LIGHTS_PER_TILE)) * u32_size;
    let grid_size = tiles * 2 * u32_size;

    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let buffer = |label: &str, size: u64, usage: wgpu::BufferUsages| {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage,
            mapped_at_creation: false,
        })
    };
    let uniform_buffer = buffer(
        "culling uniforms",
        UNIFORM_BUFFER_SIZE,
        wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    );
    let light_buffer = buffer(
        "lights",
        LIGHT_BUFFER_SIZE,
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    );
    let index_buffer = buffer(
        "light index list",
        index_size,
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
    );
    let grid_buffer = buffer(
        "light grid",
        grid_size,
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
    );
    let index_readback = buffer(
        "light index list readback",
        index_size,
        wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
    );
    let grid_readback = buffer(
        "light grid readback",
        grid_size,
        wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
    );
    culler
        .queue
        .write_buffer(&uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
    if !light_bytes.is_empty() {
        culler.queue.write_buffer(&light_buffer, 0, light_bytes);
    }

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("light culling bind group"),
        layout: &culler.layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: light_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: index_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: grid_buffer.as_entire_binding(),
            },
        ],
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("light culling encoder"),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("light culling pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&culler.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(tiles_x, tiles_y, 1);
    }
    encoder.copy_buffer_to_buffer(&index_buffer, 0, &index_readback, 0, index_size);
    encoder.copy_buffer_to_buffer(&grid_buffer, 0, &grid_readback, 0, grid_size);
    culler.queue.submit([encoder.finish()]);
    if let Some(error) = pollster::block_on(scope.pop()) {
        panic!("the light culling dispatch fails validation: {error}");
    }

    let grid = read_words(device, &grid_readback);
    let index = read_words(device, &index_readback);
    let lists = (0..tiles_x * tiles_y)
        .map(|tile| {
            let (offset, count) = (grid[2 * tile as usize], grid[2 * tile as usize + 1]);
            assert_eq!(
                offset,
                tile * SHADER_LIGHTS_PER_TILE,
                "tile {tile}'s list starts at slot {offset}, not at its stride"
            );
            assert!(
                count <= SHADER_LIGHTS_PER_TILE,
                "tile {tile} lists {count} lights, more than its {SHADER_LIGHTS_PER_TILE} slots"
            );
            index[offset as usize..(offset + count) as usize].to_vec()
        })
        .collect();
    Some(TileLists {
        tiles_x,
        tiles_y,
        lists,
    })
}

pub(crate) fn read_words(device: &wgpu::Device, buffer: &wgpu::Buffer) -> Vec<u32> {
    let (sender, receiver) = mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("the device finishes the culling dispatch");
    receiver
        .recv()
        .expect("the map callback runs")
        .expect("the readback buffer maps");
    let words = buffer
        .slice(..)
        .get_mapped_range()
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    buffer.unmap();
    words
}

// ─── Behaviour ──────────────────────────────────────────────────────────────

#[test]
fn a_point_light_in_the_top_left_of_the_image_lands_in_the_top_row() {
    let camera = Camera::perspective(Vec3::ZERO, Vec3::new(0.0, 0.0, -1.0));
    let (pixel, depth, radius_px) = (tile_centre(1, 0, 16), 10.0, 2.5);
    let light = point_light(
        camera.at_pixel(pixel, depth),
        camera.radius_for_pixels(radius_px, depth),
    );
    let expected = tiles_under_disc(pixel, radius_px, 16);
    assert_eq!(expected, BTreeSet::from([(1, 0)]));

    let Some(lists) = cull(&Frame::new(camera, 16), &[light]) else {
        return;
    };
    assert_eq!(
        lists.tiles_of(0),
        expected,
        "a light seen at pixel {pixel:?} (top-left) must be listed in tile (1, 0), row 0 at the top"
    );
}

#[test]
fn a_camera_far_from_the_origin_culls_by_where_it_looks() {
    let eye = Vec3::new(100.0, 50.0, -200.0);
    let target = eye + Vec3::new(0.3, -0.2, 1.0);
    let camera = Camera::perspective(eye, target);

    let (in_front_pixel, depth, radius_px) = (tile_centre(13, 4, 16), 20.0, 2.5);
    let in_front = point_light(
        camera.at_pixel(in_front_pixel, depth),
        camera.radius_for_pixels(radius_px, depth),
    );
    let behind = point_light(eye - Camera::forward(eye, target) * 5.0, 0.5);
    let beyond_far = point_light(camera.at_pixel(tile_centre(10, 6, 16), FAR + 50.0), 1.0);
    let expected = tiles_under_disc(in_front_pixel, radius_px, 16);
    assert_eq!(expected, BTreeSet::from([(13, 4)]));

    let Some(lists) = cull(&Frame::new(camera, 16), &[in_front, behind, beyond_far]) else {
        return;
    };
    assert_eq!(
        lists.tiles_of(0),
        expected,
        "a light in front of a camera at {eye:?} lands in the tile its centre projects to"
    );
    assert_eq!(
        lists.tiles_of(1),
        BTreeSet::new(),
        "a light behind the camera lands in no tile"
    );
    assert_eq!(
        lists.tiles_of(2),
        BTreeSet::new(),
        "a light beyond the far plane lands in no tile"
    );
}

#[test]
fn a_light_straddling_a_tile_border_lands_in_both_neighbours() {
    let camera = Camera::perspective(Vec3::ZERO, Vec3::new(0.0, 0.0, -1.0));
    // On the border between columns 9 and 10, in the middle of row 6.
    let (pixel, depth, radius_px) = ([160.0, 104.0], 10.0, 3.0);
    let light = point_light(
        camera.at_pixel(pixel, depth),
        camera.radius_for_pixels(radius_px, depth),
    );
    let expected = tiles_under_disc(pixel, radius_px, 16);
    assert_eq!(expected, BTreeSet::from([(9, 6), (10, 6)]));

    let Some(lists) = cull(&Frame::new(camera, 16), &[light]) else {
        return;
    };
    assert_eq!(lists.tiles_of(0), expected);
}

#[test]
fn tiles_of_32_pixels_list_a_light_in_the_tile_its_pixel_falls_in() {
    let camera = Camera::perspective(Vec3::ZERO, Vec3::new(0.0, 0.0, -1.0));
    let (pixel, depth, radius_px) = (tile_centre(2, 1, 32), 10.0, 3.0);
    let light = point_light(
        camera.at_pixel(pixel, depth),
        camera.radius_for_pixels(radius_px, depth),
    );
    let expected = tiles_under_disc(pixel, radius_px, 32);
    assert_eq!(expected, BTreeSet::from([(2, 1)]));

    let Some(lists) = cull(&Frame::new(camera, 32), &[light]) else {
        return;
    };
    assert_eq!((lists.tiles_x, lists.tiles_y), (10, 6));
    assert_eq!(
        lists.tiles_of(0),
        expected,
        "at 32 px a light seen at pixel {pixel:?} belongs to tile (2, 1)"
    );
}

#[test]
fn an_orthographic_camera_lists_a_light_in_the_tiles_within_its_radius() {
    let eye = Vec3::new(5.0, 3.0, 20.0);
    let target = Vec3::new(5.0, 3.0, 0.0);
    // 10 px per world unit on both axes.
    let camera = Camera::orthographic(eye, target, 9.6);
    // Centre of tile (6, 3); 13 px reaches the 8-px-away neighbours, not the
    // tiles 24 px away.
    let (pixel, depth, radius_px) = (tile_centre(6, 3, 16), 20.0, 13.0);
    let light = point_light(
        camera.at_pixel(pixel, depth),
        camera.radius_for_pixels(radius_px, depth),
    );
    let expected = tiles_under_disc(pixel, radius_px, 16);
    let block: BTreeSet<(u32, u32)> = (5..=7)
        .flat_map(|tx| (2..=4).map(move |ty| (tx, ty)))
        .collect();
    assert_eq!(expected, block);

    let Some(lists) = cull(&Frame::new(camera, 16), &[light]) else {
        return;
    };
    assert_eq!(
        lists.tiles_of(0),
        expected,
        "an orthographic light at pixel {pixel:?}, {radius_px} px wide, covers tiles 5..=7 x 2..=4 only"
    );
}

#[test]
fn a_directional_light_is_in_every_tile() {
    let eye = Vec3::new(100.0, 50.0, -200.0);
    let target = eye + Vec3::new(0.3, -0.2, 1.0);
    let camera = Camera::perspective(eye, target);
    // Point lights behind the camera around it: the directional light is
    // found at index 2, past two whole `GpuLight`s, and the one after it is
    // read whole too.
    let behind = eye - Camera::forward(eye, target) * 5.0;
    let lights = [
        point_light(behind, 0.5),
        point_light(behind, 0.5),
        directional_light(),
        point_light(behind, 0.5),
    ];

    let Some(lists) = cull(&Frame::new(camera, 16), &lights) else {
        return;
    };
    let (tiles_x, tiles_y) = tile_counts(16);
    let every_tile: BTreeSet<(u32, u32)> = (0..tiles_x)
        .flat_map(|tx| (0..tiles_y).map(move |ty| (tx, ty)))
        .collect();
    assert_eq!(
        lists.tiles_of(2),
        every_tile,
        "the directional light is in every tile"
    );
    for behind_light in [0, 1, 3] {
        assert_eq!(
            lists.tiles_of(behind_light),
            BTreeSet::new(),
            "point light {behind_light}, behind the camera, lands in no tile"
        );
    }
}

#[test]
fn a_tile_lists_no_more_lights_than_the_configured_cap() {
    let camera = Camera::perspective(Vec3::ZERO, Vec3::new(0.0, 0.0, -1.0));
    let pixel = tile_centre(10, 6, 16);
    let lights: Vec<GpuLight> = (0..5)
        .map(|i| {
            let depth = 10.0 + i as f32;
            point_light(
                camera.at_pixel(pixel, depth),
                camera.radius_for_pixels(2.0, depth),
            )
        })
        .collect();
    // Uncapped, all five are listed: the cap is what keeps the count at 2.
    let Some(uncapped) = cull(&Frame::new(camera, 16), &lights) else {
        return;
    };
    assert_eq!(
        uncapped.at(10, 6).len(),
        5,
        "five lights over tile (10, 6) with the default cap list {:?}",
        uncapped.at(10, 6)
    );
    let frame = Frame {
        max_lights_per_tile: 2,
        ..Frame::new(camera, 16)
    };
    let Some(lists) = cull(&frame, &lights) else {
        return;
    };
    let listed = lists.at(10, 6);
    assert_eq!(
        listed.len(),
        2,
        "five lights over tile (10, 6) with a cap of 2 list {listed:?}"
    );
    let distinct: BTreeSet<u32> = listed.iter().copied().collect();
    assert_eq!(distinct.len(), 2, "a light is listed twice: {listed:?}");
    assert!(
        distinct.iter().all(|&i| i < 5),
        "unknown light in {listed:?}"
    );
}

#[test]
fn a_cap_above_the_shader_stride_stops_at_the_stride() {
    let camera = Camera::perspective(Vec3::ZERO, Vec3::new(0.0, 0.0, -1.0));
    let pixel = tile_centre(10, 6, 16);
    let light_count = SHADER_LIGHTS_PER_TILE + 2;
    let lights: Vec<GpuLight> = (0..light_count)
        .map(|i| {
            let depth = 10.0 + i as f32 * 0.1;
            point_light(
                camera.at_pixel(pixel, depth),
                camera.radius_for_pixels(2.0, depth),
            )
        })
        .collect();
    let frame = Frame {
        max_lights_per_tile: 2 * SHADER_LIGHTS_PER_TILE,
        ..Frame::new(camera, 16)
    };

    let Some(lists) = cull(&frame, &lights) else {
        return;
    };
    assert_eq!(
        lists.at(10, 6).len(),
        SHADER_LIGHTS_PER_TILE as usize,
        "{light_count} lights over one tile fill its {SHADER_LIGHTS_PER_TILE} slots and no more"
    );
    assert_eq!(
        lists.at(11, 6),
        &[] as &[u32],
        "the next tile's slots hold nothing of its neighbour's overflow"
    );
}

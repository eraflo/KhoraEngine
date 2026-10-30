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

//! `light_culling.wgsl` at the edges of its domain: lights on the eye or
//! inside the near plane, lights larger than the view, tile edges measured in
//! true distance on an oblique wide-angle tile, targets that are not a
//! multiple of the tile size or far from square, spot lights, no light at
//! all, more lights than the workgroup has threads, an orthographic camera
//! whose near plane is behind the eye, and rotated cameras.
//!
//! Every expectation comes from a CPU frustum built independently of the
//! shader's plane extraction: each tile's eight corners are unprojected
//! through the inverse projection into view space and the six planes are
//! built from them in `f64`. A light is expected in a tile when its sphere
//! reaches inside all six planes, and expected out when it clears one of
//! them; a light within a hair of a plane is not judged.

use std::collections::BTreeSet;

use khora_core::math::{Mat4, Vec3, Vec4, FRAC_PI_3};
use khora_core::renderer::light::CullingUniformsData;
use khora_core::renderer::GpuLight;

use super::light_culling_gpu::{culler, read_words};

/// Slots per tile in the index list (the shader's `MAX_LIGHTS_PER_TILE`).
const SHADER_LIGHTS_PER_TILE: u32 = 128;
/// The lane's culling uniform buffer is 256 bytes, bound whole.
const UNIFORM_BUFFER_SIZE: u64 = 256;
/// The lane's light buffer is 64 KiB, bound whole.
const LIGHT_BUFFER_SIZE: u64 = 64 * 1024;
/// What the output buffers hold before the dispatch: a word the shader
/// never writes, so an entry it skipped is seen.
const UNWRITTEN: u32 = 0xA5A5_A5A5;

// ─── Camera, on the CPU ─────────────────────────────────────────────────────

#[derive(Clone, Copy)]
struct View {
    view: Mat4,
    proj: Mat4,
    width: u32,
    height: u32,
}

impl View {
    #[allow(clippy::too_many_arguments)]
    fn perspective(
        eye: Vec3,
        target: Vec3,
        up: Vec3,
        fov_y: f32,
        width: u32,
        height: u32,
        near: f32,
        far: f32,
    ) -> Self {
        Self {
            view: Mat4::look_at_rh(eye, target, up).expect("the camera looks somewhere"),
            proj: Mat4::perspective_rh_zo(fov_y, width as f32 / height as f32, near, far),
            width,
            height,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn orthographic(
        eye: Vec3,
        target: Vec3,
        up: Vec3,
        half_height: f32,
        width: u32,
        height: u32,
        near: f32,
        far: f32,
    ) -> Self {
        let half_width = half_height * width as f32 / height as f32;
        Self {
            view: Mat4::look_at_rh(eye, target, up).expect("the camera looks somewhere"),
            proj: Mat4::orthographic_rh_zo(
                -half_width,
                half_width,
                -half_height,
                half_height,
                near,
                far,
            ),
            width,
            height,
        }
    }

    /// A 60° perspective looking down -Z from the origin.
    fn simple(width: u32, height: u32) -> Self {
        Self::perspective(
            Vec3::ZERO,
            Vec3::new(0.0, 0.0, -1.0),
            Vec3::Y,
            FRAC_PI_3,
            width,
            height,
            0.1,
            100.0,
        )
    }

    fn view_proj(&self) -> Mat4 {
        self.proj * self.view
    }

    fn tiles(&self, tile_size: u32) -> (u32, u32) {
        (
            self.width.div_ceil(tile_size),
            self.height.div_ceil(tile_size),
        )
    }

    /// The view-space point at NDC `(x, y, z)`.
    fn unproject_view(&self, x: f32, y: f32, z: f32) -> [f64; 3] {
        let inverse = self.proj.inverse().expect("the projection is invertible");
        let c = inverse * Vec4::new(x, y, z, 1.0);
        [
            f64::from(c.x / c.w),
            f64::from(c.y / c.w),
            f64::from(c.z / c.w),
        ]
    }

    /// NDC of pixel `pixel` (y down).
    fn ndc_of(&self, pixel: [f32; 2]) -> [f32; 2] {
        [
            2.0 * pixel[0] / self.width as f32 - 1.0,
            1.0 - 2.0 * pixel[1] / self.height as f32,
        ]
    }

    /// The view-space point seen at `pixel`, `depth` in front of the eye
    /// (negative: behind it).
    fn view_point_at_pixel(&self, pixel: [f32; 2], depth: f64) -> [f64; 3] {
        let [x, y] = self.ndc_of(pixel);
        let near = self.unproject_view(x, y, 0.0);
        let far = self.unproject_view(x, y, 1.0);
        let t = (depth - -near[2]) / (-far[2] - -near[2]);
        [
            near[0] + (far[0] - near[0]) * t,
            near[1] + (far[1] - near[1]) * t,
            near[2] + (far[2] - near[2]) * t,
        ]
    }

    fn world_of(&self, p: [f64; 3]) -> Vec3 {
        let inverse_view = self.view.inverse().expect("the view is invertible");
        inverse_view.transform_point(Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32))
    }

    fn view_space_of(&self, p: Vec3) -> [f64; 3] {
        let v = self.view.transform_point(p);
        [f64::from(v.x), f64::from(v.y), f64::from(v.z)]
    }

    /// The world point seen at `pixel`, `depth` in front of the eye.
    fn at_pixel(&self, pixel: [f32; 2], depth: f64) -> Vec3 {
        self.world_of(self.view_point_at_pixel(pixel, depth))
    }

    /// The world radius whose sphere at `pixel` and `depth` is `pixels` wide
    /// on screen, measured towards the next pixel column.
    fn radius_for_pixels(&self, pixel: [f32; 2], depth: f64, pixels: f32) -> f32 {
        let a = self.view_point_at_pixel(pixel, depth);
        let b = self.view_point_at_pixel([pixel[0] + pixels, pixel[1]], depth);
        distance(a, b) as f32
    }

    /// The six planes of tile `(tx, ty)` in view space, normals inward, unit:
    /// left, right, bottom, top, near, far.
    fn tile_planes(&self, tx: u32, ty: u32, tile_size: u32) -> [Plane; 6] {
        let ts = tile_size as f32;
        let (w, h) = (self.width as f32, self.height as f32);
        let x0 = tx as f32 * ts / w * 2.0 - 1.0;
        let x1 = (tx + 1) as f32 * ts / w * 2.0 - 1.0;
        let yt = 1.0 - ty as f32 * ts / h * 2.0;
        let yb = 1.0 - (ty + 1) as f32 * ts / h * 2.0;
        let n = |x: f32, y: f32| self.unproject_view(x, y, 0.0);
        let f = |x: f32, y: f32| self.unproject_view(x, y, 1.0);
        let (cx, cy) = ((x0 + x1) * 0.5, (yt + yb) * 0.5);
        let interior = lerp(n(cx, cy), f(cx, cy), 0.5);
        let plane = |a: [f64; 3], b: [f64; 3], c: [f64; 3]| Plane::through(a, b, c, interior);
        [
            plane(f(x0, yt), f(x0, yb), n(x0, yt)),
            plane(f(x1, yt), f(x1, yb), n(x1, yt)),
            plane(f(x0, yb), f(x1, yb), n(x0, yb)),
            plane(f(x0, yt), f(x1, yt), n(x0, yt)),
            plane(n(x0, yt), n(x1, yt), n(x0, yb)),
            plane(f(x0, yt), f(x1, yt), f(x0, yb)),
        ]
    }
}

#[derive(Clone, Copy, Debug)]
struct Plane {
    normal: [f64; 3],
    d: f64,
}

impl Plane {
    /// The plane through `a`, `b`, `c`, its normal turned towards `inside`.
    fn through(a: [f64; 3], b: [f64; 3], c: [f64; 3], inside: [f64; 3]) -> Self {
        let n = cross(sub(b, a), sub(c, a));
        let len = dot(n, n).sqrt();
        assert!(len > 0.0, "degenerate CPU tile plane");
        let normal = [n[0] / len, n[1] / len, n[2] / len];
        let mut plane = Self {
            normal,
            d: -dot(normal, a),
        };
        if plane.distance(inside) < 0.0 {
            plane = Self {
                normal: [-normal[0], -normal[1], -normal[2]],
                d: -plane.d,
            };
        }
        plane
    }

    fn distance(&self, p: [f64; 3]) -> f64 {
        dot(self.normal, p) + self.d
    }
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn lerp(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}
fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    let d = sub(a, b);
    dot(d, d).sqrt()
}

/// Whether a light belongs in a tile, by the CPU frustum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Verdict {
    In,
    Out,
    /// Within a hair of a plane: not judged.
    Unclear,
}

fn verdict(view: &View, light: &GpuLight, tx: u32, ty: u32, tile_size: u32) -> Verdict {
    if light.light_type == GpuLight::TYPE_DIRECTIONAL {
        return Verdict::In;
    }
    let centre = view.view_space_of(Vec3::new(
        light.position[0],
        light.position[1],
        light.position[2],
    ));
    let radius = f64::from(light.range);
    let hair = 1e-3 + 1e-3 * radius;
    let mut verdict = Verdict::In;
    for plane in view.tile_planes(tx, ty, tile_size) {
        let reach = plane.distance(centre) + radius;
        if reach < -hair {
            return Verdict::Out;
        }
        if reach < hair {
            verdict = Verdict::Unclear;
        }
    }
    verdict
}

/// The tiles the CPU frustum puts `light` in; panics when any tile is unclear.
fn expected_tiles(view: &View, light: &GpuLight, tile_size: u32) -> BTreeSet<(u32, u32)> {
    let (tiles_x, tiles_y) = view.tiles(tile_size);
    let mut tiles = BTreeSet::new();
    for ty in 0..tiles_y {
        for tx in 0..tiles_x {
            match verdict(view, light, tx, ty, tile_size) {
                Verdict::In => {
                    tiles.insert((tx, ty));
                }
                Verdict::Out => {}
                Verdict::Unclear => panic!(
                    "ambiguous test geometry: light at {:?} range {} grazes tile ({tx}, {ty})",
                    light.position, light.range
                ),
            }
        }
    }
    tiles
}

fn every_tile(view: &View, tile_size: u32) -> BTreeSet<(u32, u32)> {
    let (tiles_x, tiles_y) = view.tiles(tile_size);
    (0..tiles_x)
        .flat_map(|tx| (0..tiles_y).map(move |ty| (tx, ty)))
        .collect()
}

fn light_of(kind: u32, position: Vec3, range: f32) -> GpuLight {
    GpuLight {
        position: [position.x, position.y, position.z],
        range,
        light_type: kind,
        // A spot pointing away from the camera: culling must not look at it.
        direction: [0.0, 0.0, 1.0],
        inner_cone_cos: 0.95,
        outer_cone_cos: 0.9,
        ..GpuLight::default()
    }
}

fn point(position: Vec3, range: f32) -> GpuLight {
    light_of(GpuLight::TYPE_POINT, position, range)
}

// ─── The shader, on the GPU ─────────────────────────────────────────────────

/// What the dispatch produced: per tile its `(offset, count)` and the
/// light indices it listed.
struct Culled {
    tiles_x: u32,
    tiles_y: u32,
    grid: Vec<u32>,
    lists: Vec<Vec<u32>>,
}

impl Culled {
    fn at(&self, tx: u32, ty: u32) -> &[u32] {
        &self.lists[(ty * self.tiles_x + tx) as usize]
    }

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

/// Runs the shader on `lights` with `num_lights` announced (usually
/// `lights.len()`), over output buffers filled with [`UNWRITTEN`].
fn dispatch(view: &View, tile_size: u32, lights: &[GpuLight], num_lights: u32) -> Option<Culled> {
    let _turn = crate::gpu_test::exclusive_gpu();
    let culler = culler()?;
    let device = &culler.device;
    let (tiles_x, tiles_y) = view.tiles(tile_size);
    let tiles = u64::from(tiles_x * tiles_y);

    let uniforms = CullingUniformsData {
        view_projection: view.view_proj().to_cols_array_2d(),
        screen_dimensions: [view.width as f32, view.height as f32],
        tile_count: [tiles_x, tiles_y],
        num_lights,
        tile_size,
        max_lights_per_tile: SHADER_LIGHTS_PER_TILE,
        _padding: 0,
    };
    let light_bytes: &[u8] = bytemuck::cast_slice(lights);
    assert!(light_bytes.len() as u64 <= LIGHT_BUFFER_SIZE);

    let u32_size = std::mem::size_of::<u32>() as u64;
    let index_size = tiles * u64::from(SHADER_LIGHTS_PER_TILE) * u32_size;
    let grid_size = tiles * 2 * u32_size;

    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let buffer = |label: &str, size: u64, usage: wgpu::BufferUsages, prefill: bool| {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage,
            mapped_at_creation: false,
        });
        if prefill {
            let words = vec![UNWRITTEN; (size / 4) as usize];
            culler
                .queue
                .write_buffer(&buffer, 0, bytemuck::cast_slice(&words));
        }
        buffer
    };
    let uniform_buffer = buffer(
        "culling uniforms",
        UNIFORM_BUFFER_SIZE,
        wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        false,
    );
    let light_buffer = buffer(
        "lights",
        LIGHT_BUFFER_SIZE,
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        false,
    );
    let storage =
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST;
    let index_buffer = buffer("light index list", index_size, storage, true);
    let grid_buffer = buffer("light grid", grid_size, storage, true);
    let readback = wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST;
    let index_readback = buffer("index readback", index_size, readback, false);
    let grid_readback = buffer("grid readback", grid_size, readback, false);
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
            bind_entry(0, &uniform_buffer),
            bind_entry(1, &light_buffer),
            bind_entry(2, &index_buffer),
            bind_entry(3, &grid_buffer),
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
                "tile {tile}'s offset is {offset:#x}, not its stride"
            );
            assert!(
                count <= SHADER_LIGHTS_PER_TILE,
                "tile {tile} lists {count:#x} lights"
            );
            let list = index[offset as usize..(offset + count) as usize].to_vec();
            let distinct: BTreeSet<u32> = list.iter().copied().collect();
            assert_eq!(
                distinct.len(),
                list.len(),
                "tile {tile} lists a light twice: {list:?}"
            );
            assert!(
                list.iter().all(|&i| i < num_lights),
                "tile {tile} lists an unknown light: {list:?}"
            );
            list
        })
        .collect();
    Some(Culled {
        tiles_x,
        tiles_y,
        grid,
        lists,
    })
}

fn bind_entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
    }
}

fn cull(view: &View, tile_size: u32, lights: &[GpuLight]) -> Option<Culled> {
    dispatch(view, tile_size, lights, lights.len() as u32)
}

/// Every light is listed in every tile the CPU frustum puts it in, and in
/// none it puts it out of. Returns how many (light, tile) pairs were unclear.
fn assert_agrees_with_cpu(
    view: &View,
    tile_size: u32,
    lights: &[GpuLight],
    culled: &Culled,
) -> usize {
    let mut unclear = 0;
    for ty in 0..culled.tiles_y {
        for tx in 0..culled.tiles_x {
            let listed: BTreeSet<u32> = culled.at(tx, ty).iter().copied().collect();
            for (i, light) in lights.iter().enumerate() {
                let i = i as u32;
                match verdict(view, light, tx, ty, tile_size) {
                    Verdict::In => assert!(
                        listed.contains(&i),
                        "light {i} at {:?} range {} reaches tile ({tx}, {ty}) but is not listed there ({listed:?})",
                        light.position,
                        light.range
                    ),
                    Verdict::Out => assert!(
                        !listed.contains(&i),
                        "light {i} at {:?} range {} misses tile ({tx}, {ty}) but is listed there",
                        light.position,
                        light.range
                    ),
                    Verdict::Unclear => unclear += 1,
                }
            }
        }
    }
    unclear
}

// ─── Behaviour ──────────────────────────────────────────────────────────────

/// A camera far from the origin, looking obliquely.
fn far_camera(width: u32, height: u32, fov_y: f32) -> View {
    let eye = Vec3::new(100.0, 50.0, -200.0);
    View::perspective(
        eye,
        eye + Vec3::new(0.3, -0.2, 1.0),
        Vec3::Y,
        fov_y,
        width,
        height,
        0.1,
        100.0,
    )
}

#[test]
fn a_light_on_the_eye_reaches_every_tile_once_its_range_passes_the_near_plane() {
    let view = far_camera(320, 192, FRAC_PI_3);
    let eye = view.world_of([0.0; 3]);
    let lights = [
        // On the eye, reaching 1 unit: past the near plane at 0.1, in every tile.
        point(eye, 1.0),
        // On the eye, reaching 0.05: it ends before the near plane.
        point(eye, 0.05),
        // Between the eye and the near plane, reaching 0.07 deep at most.
        point(view.at_pixel([160.0, 96.0], 0.05), 0.02),
        // Between the eye and the near plane, reaching past it around the
        // top-left of the image only.
        point(view.at_pixel([24.0, 24.0], 0.09), 0.015),
    ];
    let expected: Vec<_> = lights
        .iter()
        .map(|l| expected_tiles(&view, l, 16))
        .collect();
    assert_eq!(expected[0], every_tile(&view, 16));
    assert!(expected[1].is_empty() && expected[2].is_empty());
    assert!(expected[3].contains(&(1, 1)) && !expected[3].contains(&(19, 11)));

    let Some(culled) = cull(&view, 16, &lights) else {
        return;
    };
    for (i, tiles) in expected.iter().enumerate() {
        assert_eq!(&culled.tiles_of(i as u32), tiles, "light {i}");
    }
}

#[test]
fn a_light_larger_than_the_view_is_in_every_tile() {
    let view = far_camera(320, 192, FRAC_PI_3);
    let lights = [
        // In front, swallowing the whole view volume.
        point(view.at_pixel([160.0, 96.0], 50.0), 1.0e4),
        // Behind the eye, the eye inside it.
        point(view.world_of([0.0, 0.0, 5.0]), 1.0e4),
        // Beyond the far plane, reaching back across all of it.
        point(view.at_pixel([300.0, 10.0], 150.0), 200.0),
    ];
    let Some(culled) = cull(&view, 16, &lights) else {
        return;
    };
    for i in 0..lights.len() as u32 {
        assert_eq!(
            culled.tiles_of(i),
            every_tile(&view, 16),
            "light {i} contains the whole view volume"
        );
    }
}

/// A light just outside the left edge of tile `(tx, ty)` (or above its top
/// edge), `gap` world units away from the edge's plane in true distance, on
/// a 120° camera where the shader's raw plane coefficients are far from
/// unit length.
fn light_outside_edge(view: &View, tx: u32, ty: u32, top: bool, gap: f64, range: f32) -> GpuLight {
    let ts = 16.0;
    let pixel = if top {
        [(tx as f32 + 0.5) * ts, ty as f32 * ts]
    } else {
        [tx as f32 * ts, (ty as f32 + 0.5) * ts]
    };
    let foot = view.view_point_at_pixel(pixel, 10.0);
    let plane = view.tile_planes(tx, ty, 16)[if top { 3 } else { 0 }];
    assert!(
        plane.distance(foot).abs() < 1e-4,
        "the foot lies on the edge plane"
    );
    let centre = [
        foot[0] - plane.normal[0] * gap,
        foot[1] - plane.normal[1] * gap,
        foot[2] - plane.normal[2] * gap,
    ];
    let light = point(view.world_of(centre), range);
    // Inside every other plane: only the edge plane decides.
    let planes = view.tile_planes(tx, ty, 16);
    let back = view.view_space_of(Vec3::new(
        light.position[0],
        light.position[1],
        light.position[2],
    ));
    for (i, p) in planes.iter().enumerate() {
        if i != if top { 3 } else { 0 } {
            assert!(p.distance(back) > 0.0, "the light is inside plane {i}");
        }
    }
    light
}

#[test]
fn a_tile_edge_is_measured_in_true_distance_on_a_wide_angle_camera() {
    let view = far_camera(330, 200, 120f32.to_radians());
    let range = 0.3;
    // (tile, top edge?) — a central column, an oblique corner tile, the top
    // edge of a tile in the top-right corner.
    let edges = [((10, 6), false), ((1, 1), false), ((19, 1), true)];
    let mut lights = Vec::new();
    for &((tx, ty), top) in &edges {
        lights.push(light_outside_edge(
            &view,
            tx,
            ty,
            top,
            0.9 * f64::from(range),
            range,
        ));
        lights.push(light_outside_edge(
            &view,
            tx,
            ty,
            top,
            1.1 * f64::from(range),
            range,
        ));
    }
    let Some(culled) = cull(&view, 16, &lights) else {
        return;
    };
    for (e, &((tx, ty), top)) in edges.iter().enumerate() {
        let edge = if top { "top" } else { "left" };
        let (near, far) = (2 * e as u32, 2 * e as u32 + 1);
        assert!(
            culled.at(tx, ty).contains(&near),
            "a light 0.9 x its range outside the {edge} edge of tile ({tx}, {ty}) reaches it"
        );
        assert!(
            !culled.at(tx, ty).contains(&far),
            "a light 1.1 x its range outside the {edge} edge of tile ({tx}, {ty}) does not reach it"
        );
    }
}

#[test]
fn a_target_not_a_multiple_of_the_tile_size_culls_its_partial_last_row_and_column() {
    // 21 x 13 tiles; column 20 spans pixels 320..336, row 12 spans 192..208.
    let view = View::simple(330, 200);
    assert_eq!(view.tiles(16), (21, 13));
    let at = |pixel: [f32; 2]| {
        let depth = 10.0;
        point(
            view.at_pixel(pixel, depth),
            view.radius_for_pixels(pixel, depth, 1.5),
        )
    };
    let lights = [
        // Right of the image, inside the last column's pixel span.
        at([333.0, 104.0]),
        // Right of the last column altogether.
        at([345.0, 104.0]),
        // Below the image, inside the last row's span.
        at([104.0, 205.0]),
        // Below the last row altogether.
        at([104.0, 215.0]),
        // The image's own bottom-right pixel.
        at([329.0, 199.0]),
    ];
    let expected = [
        BTreeSet::from([(20, 6)]),
        BTreeSet::new(),
        BTreeSet::from([(6, 12)]),
        BTreeSet::new(),
        BTreeSet::from([(20, 12)]),
    ];
    for (i, light) in lights.iter().enumerate() {
        assert_eq!(
            expected_tiles(&view, light, 16),
            expected[i],
            "CPU, light {i}"
        );
    }

    let Some(culled) = cull(&view, 16, &lights) else {
        return;
    };
    for (i, tiles) in expected.iter().enumerate() {
        assert_eq!(&culled.tiles_of(i as u32), tiles, "light {i}");
    }
}

#[test]
fn a_very_wide_target_culls_by_the_same_frustum_as_the_cpu() {
    // A 25:1 strip: 63 x 3 tiles at 16 px, 32 x 2 at 32 px, a ~162° horizontal view.
    let view = View::perspective(
        Vec3::new(2.0, 1.0, 3.0),
        Vec3::new(2.0, 1.0, -7.0),
        Vec3::Y,
        30f32.to_radians(),
        1000,
        40,
        0.1,
        100.0,
    );
    for tile_size in [16, 32] {
        let lights: Vec<GpuLight> = [
            [5.0, 5.0],
            [500.0, 20.0],
            [995.0, 35.0],
            [999.5, 39.5],
            [250.0, 30.0],
            [760.0, 8.0],
        ]
        .iter()
        .enumerate()
        .map(|(i, &pixel)| {
            let depth = 4.0 + 3.0 * i as f64;
            point(
                view.at_pixel(pixel, depth),
                view.radius_for_pixels(pixel, depth, 5.0),
            )
        })
        .collect();
        let Some(culled) = cull(&view, tile_size, &lights) else {
            return;
        };
        assert_eq!(
            (culled.tiles_x, culled.tiles_y),
            view.tiles(tile_size),
            "{tile_size} px"
        );
        let unclear = assert_agrees_with_cpu(&view, tile_size, &lights, &culled);
        assert!(
            unclear < 4,
            "{unclear} unclear (light, tile) pairs at {tile_size} px"
        );
        for i in 0..lights.len() as u32 {
            assert!(
                !culled.tiles_of(i).is_empty(),
                "light {i} on screen is listed somewhere"
            );
        }
    }
}

#[test]
fn a_spot_light_is_culled_as_the_sphere_of_its_range() {
    let view = far_camera(320, 192, FRAC_PI_3);
    let positions: Vec<(Vec3, f32)> = [[24.0, 24.0], [160.0, 96.0], [300.0, 180.0], [96.0, 150.0]]
        .iter()
        .enumerate()
        .map(|(i, &pixel)| {
            let depth = 6.0 + 4.0 * i as f64;
            (
                view.at_pixel(pixel, depth),
                view.radius_for_pixels(pixel, depth, 21.0),
            )
        })
        .collect();
    let mut lights: Vec<GpuLight> = positions
        .iter()
        .map(|&(p, r)| light_of(GpuLight::TYPE_SPOT, p, r))
        .collect();
    lights.extend(positions.iter().map(|&(p, r)| point(p, r)));

    let Some(culled) = cull(&view, 16, &lights) else {
        return;
    };
    let spots = positions.len() as u32;
    for i in 0..spots {
        let tiles = culled.tiles_of(i);
        assert_eq!(
            tiles,
            expected_tiles(&view, &lights[i as usize], 16),
            "spot {i} covers its range sphere's tiles"
        );
        assert_eq!(
            tiles,
            culled.tiles_of(spots + i),
            "spot {i} and the point light in its place"
        );
        assert!(tiles.len() > 1, "spot {i} spans several tiles");
    }
}

#[test]
fn no_light_leaves_every_tile_empty_with_its_offset_written() {
    let view = View::simple(330, 200);
    // Lights sit in the buffer, but the frame announces none of them.
    let stale = [
        point(view.at_pixel([165.0, 100.0], 10.0), 1.0e3),
        light_of(GpuLight::TYPE_DIRECTIONAL, Vec3::ZERO, 0.0),
    ];
    for lights in [&[][..], &stale[..]] {
        let Some(culled) = dispatch(&view, 16, lights, 0) else {
            return;
        };
        for tile in 0..culled.tiles_x * culled.tiles_y {
            assert_eq!(
                culled.grid[2 * tile as usize],
                tile * SHADER_LIGHTS_PER_TILE,
                "tile {tile}'s offset is written"
            );
            assert_eq!(
                culled.grid[2 * tile as usize + 1],
                0,
                "tile {tile} lists nothing"
            );
        }
    }
}

#[test]
fn every_light_past_the_workgroup_size_is_culled_into_its_tile() {
    // 21 x 13 = 273 tiles; 300 lights, one per tile then 27 more, so lights
    // 256..300 are read on the threads' second pass.
    let view = View::simple(330, 200);
    let (tiles_x, tiles_y) = view.tiles(16);
    let tiles = tiles_x * tiles_y;
    let lights: Vec<GpuLight> = (0..300u32)
        .map(|i| {
            let tile = i % tiles;
            let pixel = [
                ((tile % tiles_x) as f32 + 0.5) * 16.0,
                ((tile / tiles_x) as f32 + 0.5) * 16.0,
            ];
            let depth = 5.0 + f64::from(i % 7);
            point(
                view.at_pixel(pixel, depth),
                view.radius_for_pixels(pixel, depth, 2.0),
            )
        })
        .collect();
    let Some(culled) = cull(&view, 16, &lights) else {
        return;
    };
    for ty in 0..tiles_y {
        for tx in 0..tiles_x {
            let tile = ty * tiles_x + tx;
            let mut expected: BTreeSet<u32> = BTreeSet::from([tile]);
            if tile + tiles < 300 {
                expected.insert(tile + tiles);
            }
            let listed: BTreeSet<u32> = culled.at(tx, ty).iter().copied().collect();
            assert_eq!(listed, expected, "tile ({tx}, {ty})");
        }
    }
}

#[test]
fn an_orthographic_camera_with_its_near_plane_behind_the_eye_keeps_the_lights_between() {
    // Khora's default orthographic near plane is -1: one unit behind the eye.
    let view = View::orthographic(
        Vec3::new(5.0, 3.0, 20.0),
        Vec3::new(5.0, 3.0, 0.0),
        Vec3::Y,
        9.6,
        320,
        192,
        -1.0,
        100.0,
    );
    let pixel = [104.0, 56.0]; // centre of tile (6, 3)
    let lights = [
        // Half a unit behind the eye, inside the view volume.
        point(view.at_pixel(pixel, -0.5), 0.2),
        // Two units behind, reaching 1.5 behind: outside.
        point(view.at_pixel(pixel, -2.0), 0.5),
        // 1.3 behind, reaching 0.8 behind: inside.
        point(view.at_pixel(pixel, -1.3), 0.5),
    ];
    let expected = [
        BTreeSet::from([(6, 3)]),
        BTreeSet::new(),
        BTreeSet::from([(6, 3)]),
    ];
    for (i, light) in lights.iter().enumerate() {
        assert_eq!(
            expected_tiles(&view, light, 16),
            expected[i],
            "CPU, light {i}"
        );
    }
    let Some(culled) = cull(&view, 16, &lights) else {
        return;
    };
    for (i, tiles) in expected.iter().enumerate() {
        assert_eq!(&culled.tiles_of(i as u32), tiles, "light {i}");
    }
}

#[test]
fn a_rolled_or_downward_camera_culls_in_its_own_image_axes() {
    let rolled = View::perspective(
        Vec3::new(1.0, 2.0, 3.0),
        Vec3::new(1.0, 2.0, -7.0),
        Vec3::X, // rolled 90°: world +X is image up
        FRAC_PI_3,
        320,
        192,
        0.1,
        100.0,
    );
    let down = View::perspective(
        Vec3::new(4.0, 10.0, -2.0),
        Vec3::new(4.0, 0.0, -2.0),
        Vec3::new(0.0, 0.0, -1.0),
        FRAC_PI_3,
        320,
        192,
        0.1,
        100.0,
    );
    for (name, view) in [("rolled", rolled), ("downward", down)] {
        let lights: Vec<GpuLight> = [[24.0, 8.0], [296.0, 24.0], [40.0, 184.0], [168.0, 104.0]]
            .iter()
            .map(|&pixel| {
                point(
                    view.at_pixel(pixel, 6.0),
                    view.radius_for_pixels(pixel, 6.0, 3.0),
                )
            })
            .collect();
        let expected: Vec<_> = lights
            .iter()
            .map(|l| expected_tiles(&view, l, 16))
            .collect();
        assert_eq!(
            expected[0],
            BTreeSet::from([(1, 0)]),
            "{name}: top-left light"
        );
        assert_eq!(
            expected[2],
            BTreeSet::from([(2, 11)]),
            "{name}: bottom-left light"
        );
        let Some(culled) = cull(&view, 16, &lights) else {
            return;
        };
        for (i, tiles) in expected.iter().enumerate() {
            assert_eq!(
                &culled.tiles_of(i as u32),
                tiles,
                "{name} camera, light {i}"
            );
        }
    }
}

#[test]
fn a_light_count_past_the_light_buffer_lists_only_the_lights_it_holds() {
    // The lane's 64 KiB light buffer holds 819 lights of 80 bytes; a frame
    // with more announces them all (`num_lights: lights.len()`) while its
    // upload is refused as out of bounds.
    let capacity = (LIGHT_BUFFER_SIZE / std::mem::size_of::<GpuLight>() as u64) as u32;
    assert_eq!(capacity, 819);
    let view = View::simple(320, 192);
    // Every stored light is behind the camera: no tile should list any.
    let behind = point(view.world_of([0.0, 0.0, 5.0]), 0.5);
    let lights = vec![behind; capacity as usize];
    let Some(culled) = dispatch(&view, 16, &lights, capacity + 81) else {
        return;
    };
    for ty in 0..culled.tiles_y {
        for tx in 0..culled.tiles_x {
            assert_eq!(
                culled.at(tx, ty),
                &[] as &[u32],
                "tile ({tx}, {ty}) lists lights the buffer does not hold"
            );
        }
    }
}

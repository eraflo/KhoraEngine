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

//! Sandbox example demonstrating high-level game logic with Khora Engine.
//!
//! This example shows how to build a game using only the SDK's public API.
//! No low-level rendering details - pure game logic.
//!
//! Controls:
//! - Right mouse button + drag: Look around
//! - W/A/S/D: Move forward/left/backward/right
//! - Space: Move up
//! - Shift: Move down

use anyhow::Result;
use khora_sdk::khora_core::platform::{InputBinding, InputMap};
use khora_sdk::prelude::math::{Quaternion, Vec3};
use khora_sdk::prelude::*;
use khora_sdk::run_winit;
use khora_sdk::winit_adapters::WinitWindowProvider;
use khora_sdk::{
    AgentProvider, AudioDevice, AudioMixBus, CpalAudioDevice, DccService, DefaultMixBus, EngineApp,
    GameWorld, InputEvent, KeyCode, LayoutSystem, PhaseProvider, PhysicsProvider, PipelineSystem,
    RapierPhysicsWorld, RenderSystem, Runtime, StandardTextRenderer, StreamInfo, TaffyLayoutSystem,
    TextRenderer, WgpuPipelineSystem, WgpuRenderSystem, WindowConfig, TEXT_WGSL,
};
use std::sync::{Arc, Mutex};

#[global_allocator]
static GLOBAL: SaaTrackingAllocator = SaaTrackingAllocator::new(std::alloc::System);

// Action names used by the player controller. Centralised so both the
// `bind` calls and the `is_pressed` queries point at the same strings.
const ACTION_FORWARD: &str = "player.forward";
const ACTION_BACKWARD: &str = "player.backward";
const ACTION_LEFT: &str = "player.left";
const ACTION_RIGHT: &str = "player.right";
const ACTION_UP: &str = "player.up";
const ACTION_DOWN: &str = "player.down";

/// Simple camera controller for the player.
///
/// Movement is driven through the engine-wide [`InputMap`] (action /
/// binding map). Mouse look stays on raw [`InputEvent`]s because mouse
/// motion isn't representable as an action — the InputMap is for boolean
/// inputs (held / just-pressed / just-released).
struct PlayerController {
    speed: f32,
    sensitivity: f32,
    yaw: f32,
    pitch: f32,
    mouse_captured: bool,
    last_mouse: (f32, f32),
}

impl PlayerController {
    fn new() -> Self {
        Self {
            speed: 5.0,
            sensitivity: 0.003,
            yaw: std::f32::consts::PI,
            pitch: 0.0,
            mouse_captured: false,
            last_mouse: (0.0, 0.0),
        }
    }

    /// Registers the controller's action bindings with the engine
    /// [`InputMap`]. Called once at setup. Multiple bindings per action
    /// (e.g. arrows AND WASD) demonstrate the OR semantics.
    fn bind_actions(map: &mut InputMap) {
        map.bind(ACTION_FORWARD, InputBinding::Key(KeyCode::KeyW));
        map.bind(ACTION_FORWARD, InputBinding::Key(KeyCode::ArrowUp));
        map.bind(ACTION_BACKWARD, InputBinding::Key(KeyCode::KeyS));
        map.bind(ACTION_BACKWARD, InputBinding::Key(KeyCode::ArrowDown));
        map.bind(ACTION_LEFT, InputBinding::Key(KeyCode::KeyA));
        map.bind(ACTION_LEFT, InputBinding::Key(KeyCode::ArrowLeft));
        map.bind(ACTION_RIGHT, InputBinding::Key(KeyCode::KeyD));
        map.bind(ACTION_RIGHT, InputBinding::Key(KeyCode::ArrowRight));
        map.bind(ACTION_UP, InputBinding::Key(KeyCode::Space));
        map.bind(ACTION_DOWN, InputBinding::Key(KeyCode::ShiftLeft));
        map.bind(ACTION_DOWN, InputBinding::Key(KeyCode::ShiftRight));
    }

    /// Drives mouse look from raw events. Movement keys are queried from
    /// the engine's [`InputMap`] in [`Self::movement_axes`] so this loop
    /// only handles things the InputMap can't model.
    fn process_input(&mut self, inputs: &[InputEvent]) {
        use khora_sdk::prelude::MouseButton;

        for event in inputs {
            match event {
                InputEvent::MouseButtonPressed { button } if *button == MouseButton::Right => {
                    self.mouse_captured = true;
                }
                InputEvent::MouseButtonReleased { button } if *button == MouseButton::Right => {
                    self.mouse_captured = false;
                }
                InputEvent::MouseMoved { x, y } => {
                    if self.mouse_captured {
                        let dx = x - self.last_mouse.0;
                        let dy = y - self.last_mouse.1;

                        self.yaw -= dx * self.sensitivity;
                        self.pitch -= dy * self.sensitivity;
                        self.pitch = self.pitch.clamp(
                            -std::f32::consts::FRAC_PI_2 + 0.01,
                            std::f32::consts::FRAC_PI_2 - 0.01,
                        );
                    }
                    self.last_mouse = (*x, *y);
                }
                _ => {}
            }
        }
    }

    /// Resolves the current movement axes from the [`InputMap`].
    /// Returns `(forward, right, up)` in {-1, 0, 1} per axis.
    fn movement_axes(input_map: &InputMap) -> (f32, f32, f32) {
        let mut forward = 0.0;
        let mut right = 0.0;
        let mut up = 0.0;
        if input_map.is_pressed(ACTION_FORWARD) {
            forward -= 1.0;
        }
        if input_map.is_pressed(ACTION_BACKWARD) {
            forward += 1.0;
        }
        if input_map.is_pressed(ACTION_LEFT) {
            right -= 1.0;
        }
        if input_map.is_pressed(ACTION_RIGHT) {
            right += 1.0;
        }
        if input_map.is_pressed(ACTION_UP) {
            up += 1.0;
        }
        if input_map.is_pressed(ACTION_DOWN) {
            up -= 1.0;
        }
        (forward, right, up)
    }

    fn update(
        &self,
        transform: &mut khora_sdk::prelude::ecs::Transform,
        delta_time: f32,
        input_map: &InputMap,
    ) {
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();
        let (sin_pitch, cos_pitch) = self.pitch.sin_cos();

        let forward = Vec3::new(sin_yaw * cos_pitch, sin_pitch, cos_yaw * cos_pitch);
        let right = Vec3::new(cos_yaw, 0.0, -sin_yaw);

        let (move_forward, move_right, move_up) = Self::movement_axes(input_map);

        let velocity = self.speed * delta_time;
        transform.translation = transform.translation
            + forward * (-move_forward) * velocity
            + right * move_right * velocity
            + Vec3::Y * move_up * velocity;

        let yaw_quat = Quaternion::from_axis_angle(Vec3::Y, self.yaw);
        let pitch_quat = Quaternion::from_axis_angle(Vec3::X, self.pitch);
        transform.rotation = yaw_quat * pitch_quat;
    }
}

/// A simple game demonstrating the Khora SDK.
struct SandboxGame {
    frame_count: u64,
    player: Option<khora_sdk::prelude::ecs::EntityId>,
    controller: PlayerController,
    /// Cached handle to the engine's InputMap. Stored at `setup` time so
    /// `update` (which doesn't receive `&ServiceRegistry`) can still query
    /// actions each frame.
    input_map: Option<Arc<Mutex<InputMap>>>,
    /// Cached handle to the engine's per-frame [`Time`] resource. Cached at
    /// `setup` time so `update` can read the real frame delta (the scheduler
    /// publishes it each frame) instead of a hardcoded constant.
    time: Option<khora_sdk::prelude::SharedTime>,
}

impl EngineApp for SandboxGame {
    fn window_config() -> WindowConfig {
        WindowConfig {
            title: "Khora Sandbox".to_owned(),
            ..WindowConfig::default()
        }
    }

    fn new() -> Self {
        log::info!("SandboxGame: Initializing...");
        Self {
            frame_count: 0,
            player: None,
            controller: PlayerController::new(),
            input_map: None,
            time: None,
        }
    }

    fn setup(&mut self, world: &mut GameWorld, runtime: &Runtime) {
        // Cache the InputMap handle and register the player movement bindings.
        if let Some(map_arc) = runtime.resources.get::<Arc<Mutex<InputMap>>>() {
            if let Ok(mut map) = map_arc.lock() {
                PlayerController::bind_actions(&mut map);
            }
            self.input_map = Some(map_arc.clone());
        }

        // Cache the per-frame Time handle so `update` reads the real frame
        // delta instead of a hardcoded step.
        self.time = runtime
            .resources
            .get::<khora_sdk::prelude::SharedTime>()
            .cloned();

        let camera = khora_sdk::prelude::ecs::Camera::new_perspective(
            std::f32::consts::FRAC_PI_4,
            16.0 / 9.0,
            0.1,
            1000.0,
        );
        self.player = Some(
            khora_sdk::Vessel::at(world, Vec3::new(0.0, 2.0, 10.0))
                .with_component(camera)
                .with_rotation(Quaternion::from_axis_angle(Vec3::Y, std::f32::consts::PI))
                .build(),
        );

        // The ground needs an explicit material: the projection has no
        // default-material fallback, so a mesh without a material handle is
        // skipped (and logged). A matte grey, no texture map.
        let ground_mat = khora_sdk::prelude::materials::StandardMaterial {
            base_color: khora_sdk::prelude::math::LinearRgba::new(0.5, 0.5, 0.5, 1.0),
            roughness: 0.9,
            ..Default::default()
        };
        let ground_handle = world.add_material(ground_mat);
        khora_sdk::spawn_plane(world, 20.0, 0.0)
            .with_component(ground_handle)
            .build();

        let sun_rotation = Quaternion::from_axis_angle(Vec3::X, -std::f32::consts::FRAC_PI_2 * 0.8);
        let mut sun_light = khora_sdk::prelude::ecs::Light::directional();
        if let khora_sdk::prelude::ecs::LightType::Directional(ref mut d) = sun_light.light_type {
            d.intensity = 2.5;
            d.shadow_enabled = true;
            d.shadow_bias = 0.005;
            d.shadow_normal_bias = 0.02;
        }

        khora_sdk::Vessel::at(world, Vec3::new(0.0, 20.0, 5.0))
            .with_component(sun_light)
            .with_rotation(sun_rotation)
            .build();

        // Register a procedural checkerboard texture in the shared AssetStore
        // (CpuTexture sub-store); the material projection uploads it to the GPU
        // and every lit lane samples it as the albedo map.
        let checker_uuid = register_checker_texture(runtime);

        // Procedural HDR environment, projected onto the IBL environment cube
        // by the bake. `register_agents` points `EnvironmentMap` at this id.
        register_environment_map(runtime);

        let positions = [
            Vec3::new(0.0, 0.5, -5.0),
            Vec3::new(-3.0, 0.5, -8.0),
            Vec3::new(3.0, 0.5, -6.0),
            Vec3::new(-1.5, 0.5, -4.0),
            Vec3::new(2.0, 0.5, -10.0),
        ];
        let colors = [
            khora_sdk::prelude::math::LinearRgba::RED,
            khora_sdk::prelude::math::LinearRgba::GREEN,
            khora_sdk::prelude::math::LinearRgba::BLUE,
            khora_sdk::prelude::math::LinearRgba::YELLOW,
            khora_sdk::prelude::math::LinearRgba::CYAN,
        ];

        let mut point_light = khora_sdk::prelude::ecs::Light::point();
        if let khora_sdk::prelude::ecs::LightType::Point(ref mut p) = point_light.light_type {
            // `intensity` is a direct linear multiplier on the light color in
            // this shading model (not a physical luminous power), so it pairs
            // with the windowed `(1 - (d/range)^2)^2` attenuation. A few units
            // is bright; large values saturate every nearby surface to white
            // after the Reinhard tonemap.
            p.intensity = 5.0;
            p.color = khora_sdk::prelude::math::LinearRgba::new(0.8, 0.9, 1.0, 1.0);
            p.range = 15.0;
        }
        world.spawn((
            khora_sdk::prelude::ecs::Transform::from_translation(Vec3::new(0.0, 1.0, -2.0)),
            khora_sdk::prelude::ecs::GlobalTransform::default(),
            point_light,
        ));

        for (i, pos) in positions.iter().enumerate() {
            let mat = khora_sdk::prelude::materials::StandardMaterial {
                base_color: colors[i],
                base_color_texture: Some(checker_uuid),
                roughness: 0.2,
                ..Default::default()
            };
            let mat_handle = world.add_material(*Box::new(mat));

            khora_sdk::spawn_sphere(world, 0.75, 32, 16)
                .at_position(*pos)
                .with_component(mat_handle)
                .build();
        }

        // Chrome ball — a smooth metal sphere mirrors the environment almost
        // directly, so it shows what the IBL specular chain is actually
        // sampling (sun, light panels, sky/ground split) rather than the soft
        // sheen a rough dielectric gives.
        let chrome = khora_sdk::prelude::materials::StandardMaterial {
            base_color: khora_sdk::prelude::math::LinearRgba::rgb(0.95, 0.96, 0.98),
            metallic: 1.0,
            roughness: 0.05,
            ..Default::default()
        };
        let chrome_handle = world.add_material(*Box::new(chrome));
        khora_sdk::spawn_sphere(world, 0.9, 48, 24)
            .at_position(Vec3::new(0.0, 0.9, -2.6))
            .with_component(chrome_handle)
            .build();

        // Two tinted glass spheres, one behind the other, to show alpha
        // blending: `AlphaMode::Blend` puts them in the depth-sorted
        // transparent batch, so the far one shows through the near one and the
        // scene behind shows through both. Overlapping them is the actual test
        // — a single transparent object would look right even unsorted.
        for (offset, tint) in [
            (
                0.0_f32,
                khora_sdk::prelude::math::LinearRgba::new(0.25, 0.85, 0.55, 0.35),
            ),
            (
                1.1,
                khora_sdk::prelude::math::LinearRgba::new(0.95, 0.55, 0.25, 0.35),
            ),
        ] {
            let glass = khora_sdk::prelude::materials::StandardMaterial {
                base_color: tint,
                alpha_mode: khora_sdk::prelude::materials::AlphaMode::Blend,
                // Glass is smooth and double-sided: without back faces the
                // sphere would look hollow once you can see into it.
                roughness: 0.1,
                double_sided: true,
                ..Default::default()
            };
            let glass_handle = world.add_material(*Box::new(glass));
            khora_sdk::spawn_sphere(world, 0.8, 32, 16)
                .at_position(Vec3::new(-2.2 + offset, 0.8, -1.4 - offset))
                .with_component(glass_handle)
                .build();
        }
    }

    fn update(&mut self, world: &mut GameWorld, inputs: &[InputEvent]) {
        self.frame_count += 1;

        self.controller.process_input(inputs);

        // Real wall-clock delta from the engine's Time resource — gameplay
        // movement is variable-rate, so it uses `delta_seconds` (not the fixed
        // sim step). Falls back to a 60 Hz step if the resource is unavailable.
        let dt = self
            .time
            .as_ref()
            .and_then(|t| t.read().ok().map(|t| t.delta_seconds))
            .unwrap_or(1.0 / 60.0);

        if let Some(player) = self.player {
            if let Some(transform) = world.get_transform_mut(player) {
                if let Some(map_arc) = &self.input_map {
                    if let Ok(map) = map_arc.lock() {
                        self.controller.update(transform, dt, &map);
                    }
                }
            }
            world.sync_global_transform(player);
        }

        if self.frame_count.is_multiple_of(300) {
            let entity_count = world.iter_entities().count();
            let mouse = if self.controller.mouse_captured {
                "captured"
            } else {
                "free"
            };
            log::info!(
                "SandboxGame: Frame {}, {} entities, mouse: {}",
                self.frame_count,
                entity_count,
                mouse
            );
        }
    }
}

impl AgentProvider for SandboxGame {
    fn register_agents(&self, _dcc: &DccService, runtime: &mut Runtime) {
        // No custom agents. This is, however, the engine's sanctioned runtime
        // *mutation* point (`setup` only gets `&Runtime`), so the scene's
        // environment selection is inserted here — before the first tick, and
        // therefore before the one-time IBL bake reads it.
        let env_uuid = khora_sdk::khora_core::asset::AssetUUID::new_v5(ENV_ASSET_NAME);
        runtime
            .resources
            .insert(khora_sdk::EnvironmentMap::from_asset(env_uuid));
    }
}

impl PhaseProvider for SandboxGame {
    fn custom_phases(&self) -> Vec<khora_sdk::ExecutionPhase> {
        Vec::new()
    }

    fn removed_phases(&self) -> Vec<khora_sdk::ExecutionPhase> {
        Vec::new()
    }
}

/// Builds a small procedural checkerboard [`CpuTexture`] and inserts it into
/// the shared AssetStore CpuTexture sub-store (filled by the SDK layer, uploaded to the GPU
/// by the data-layer material projection). Returns the texture's `AssetUUID`
/// to attach to a material's `base_color_texture`.
fn register_checker_texture(runtime: &Runtime) -> khora_sdk::khora_core::asset::AssetUUID {
    use khora_sdk::khora_core::asset::{AssetHandle, AssetUUID};
    use khora_sdk::khora_core::math::Extent3D;
    use khora_sdk::khora_core::renderer::api::resource::{
        CpuTexture, TextureDimension, TextureUsage,
    };
    use khora_sdk::khora_core::renderer::api::util::{SampleCount, TextureFormat};

    const N: u32 = 8;
    let mut pixels = Vec::with_capacity((N * N * 4) as usize);
    for y in 0..N {
        for x in 0..N {
            let lit = (x + y) % 2 == 0;
            let v = if lit { 235u8 } else { 60u8 };
            pixels.extend_from_slice(&[v, v, v, 255]);
        }
    }

    let cpu = CpuTexture {
        pixels,
        size: Extent3D {
            width: N,
            height: N,
            depth_or_array_layers: 1,
        },
        // Pixel layout only — the material slot that binds this texture
        // supplies the color space (albedo reads it as sRGB).
        format: TextureFormat::Rgba8Unorm,
        mip_level_count: 1,
        sample_count: SampleCount::X1,
        dimension: TextureDimension::D2,
        usage: TextureUsage::TEXTURE_BINDING | TextureUsage::COPY_DST,
    };

    let uuid = AssetUUID::new_v5("sandbox/checker_albedo");
    if let Some(assets) = runtime.resources.get::<khora_sdk::khora_data::AssetStore>() {
        assets
            .store::<CpuTexture>()
            .write()
            .unwrap()
            .insert(uuid, AssetHandle::new(cpu));
    } else {
        log::warn!("sandbox: AssetStore not found; checker texture not registered");
    }
    uuid
}

/// Stable id of the sandbox's procedural environment map.
const ENV_ASSET_NAME: &str = "sandbox/studio_env";

/// Builds a procedural **equirectangular HDR** environment and registers it as
/// a `CpuTexture` asset, standing in for an authored `.hdr` until one is
/// dropped in.
///
/// This is a "studio" environment: a sky/ground gradient plus a sun and three
/// tinted light panels, all at radiances well above 1.0. The high dynamic range
/// is the point — it is what makes the panels read as crisp bright reflections
/// on glossy surfaces instead of washing into the background, and an 8-bit
/// image could not represent it.
///
/// The direction → pixel mapping is the exact inverse of the one
/// `ibl_equirect.wgsl` applies when projecting onto the cube: longitude from
/// `atan2(z, x)`, latitude from the polar angle with `v = 0` at `+Y`.
fn register_environment_map(runtime: &Runtime) -> khora_sdk::khora_core::asset::AssetUUID {
    use khora_sdk::khora_core::asset::{AssetHandle, AssetUUID};
    use khora_sdk::khora_core::renderer::api::resource::CpuTexture;

    const W: u32 = 1024;
    const H: u32 = 512;
    const PI: f32 = std::f32::consts::PI;

    // Angular half-extents of a light panel (~20° x 11°, a credible softbox),
    // the share of that extent held at full radiance, and the panels as
    // (azimuth, elevation, linear radiance). Distinct tints make it obvious
    // which reflection comes from where.
    const PANEL_AZ_HALF: f32 = 0.18;
    const PANEL_EL_HALF: f32 = 0.10;
    // Normalised distance out to which the panel stays at full intensity;
    // beyond it the edge feathers to zero. A panel that fades from its very
    // centre has no crisp core to reflect and reads as a coloured cloud.
    const PANEL_CORE: f32 = 0.6;
    let panels = [
        (-2.2_f32, 0.45_f32, [22.0_f32, 17.0, 12.0]), // warm key light
        (1.1, 0.30, [4.0, 12.0, 26.0]),               // cool fill
        (2.6, 0.55, [9.0, 2.0, 7.0]),                 // magenta rim (accent only)
    ];
    // Sun placed to agree with the scene's directional light (which points
    // down and slightly toward -Z, so the sun sits high toward +Z).
    let sun_dir = [0.0_f32, 0.95, 0.31];
    let sun_cos = 0.9986_f32; // ~3° radius

    let mut rgba = Vec::with_capacity((W * H * 4) as usize);
    for y in 0..H {
        let v = (y as f32 + 0.5) / H as f32;
        let theta = v * PI; // polar angle from +Y
        let (sin_t, cos_t) = theta.sin_cos();
        for x in 0..W {
            let u = (x as f32 + 0.5) / W as f32;
            let phi = (u - 0.5) * 2.0 * PI;
            let (sin_p, cos_p) = phi.sin_cos();
            let dir = [sin_t * cos_p, cos_t, sin_t * sin_p];

            // Sky above the horizon, dim warm ground below.
            let up = dir[1];
            let mut color = if up >= 0.0 {
                let t = up.powf(0.4);
                [
                    0.45 + (0.05 - 0.45) * t,
                    0.55 + (0.13 - 0.55) * t,
                    0.70 + (0.42 - 0.70) * t,
                ]
            } else {
                let t = (-up * 3.0).clamp(0.0, 1.0);
                [
                    0.45 + (0.05 - 0.45) * t,
                    0.55 + (0.05 - 0.55) * t,
                    0.70 + (0.04 - 0.70) * t,
                ]
            };

            // Light panels — angular rectangles with a soft edge.
            let elevation = up.clamp(-1.0, 1.0).asin();
            for (paz, pel, radiance) in &panels {
                let mut daz = phi - paz;
                while daz > PI {
                    daz -= 2.0 * PI;
                }
                while daz < -PI {
                    daz += 2.0 * PI;
                }
                // Box metric: 0 at the panel's centre, 1 at its edge. Taking the
                // max of the two axes keeps the panel rectangular — a softbox
                // shape — instead of an ellipse.
                let nx = (daz.abs() / PANEL_AZ_HALF).clamp(0.0, 1.0);
                let ny = ((elevation - pel).abs() / PANEL_EL_HALF).clamp(0.0, 1.0);
                let d = nx.max(ny);
                // Full intensity across the core, then a smoothstep edge — a
                // hard cut would alias when projected onto the cube faces.
                let t = ((d - PANEL_CORE) / (1.0 - PANEL_CORE)).clamp(0.0, 1.0);
                let f = 1.0 - t * t * (3.0 - 2.0 * t);
                if f > 0.0 {
                    for c in 0..3 {
                        color[c] += radiance[c] * f;
                    }
                }
            }

            // Sun disk with a tight halo.
            let mu = dir[0] * sun_dir[0] + dir[1] * sun_dir[1] + dir[2] * sun_dir[2];
            if mu > 0.0 {
                let disk = if mu >= sun_cos { 60.0 } else { 0.0 };
                let halo = mu.powf(400.0) * 6.0;
                let sun = disk + halo;
                color[0] += sun;
                color[1] += sun * 0.95;
                color[2] += sun * 0.85;
            }

            rgba.extend_from_slice(&[color[0], color[1], color[2], 1.0]);
        }
    }

    let uuid = AssetUUID::new_v5(ENV_ASSET_NAME);
    let Some(cpu) = CpuTexture::from_rgba32f(W, H, &rgba) else {
        log::error!("sandbox: environment map has a mismatched pixel count");
        return uuid;
    };
    if let Some(assets) = runtime.resources.get::<khora_sdk::khora_data::AssetStore>() {
        assets
            .store::<CpuTexture>()
            .write()
            .unwrap()
            .insert(uuid, AssetHandle::new(cpu));
        log::info!("sandbox: registered procedural HDR environment ({W}x{H} equirectangular)");
    } else {
        log::warn!("sandbox: AssetStore not found; environment map not registered");
    }
    uuid
}

fn main() -> Result<()> {
    use env_logger::{Builder, Env};

    Builder::from_env(Env::default().default_filter_or("info"))
        // Suppress Epic Games / EOS overlay Vulkan loader JSON-not-found noise.
        // These are harmless OS-level loader warnings, not engine errors.
        .filter_module("wgpu_hal::vulkan::instance", log::LevelFilter::Off)
        .init();

    run_winit::<WinitWindowProvider, SandboxGame>(|window, runtime, _event_loop| {
        let mut rs = WgpuRenderSystem::new();
        rs.init(window).expect("renderer init failed");
        // Register the graphics device before boxing — required by RenderAgent.
        runtime.backends.insert(rs.graphics_device());
        let rs: Box<dyn RenderSystem> = Box::new(rs);
        runtime.backends.insert(Arc::new(Mutex::new(rs)));

        // Shader / pipeline backend — wgpu + naga_oil. The app picks the
        // backend; the engine core consumes it as `Arc<dyn PipelineSystem>`.
        match WgpuPipelineSystem::new() {
            Ok(sys) => {
                let sys: Arc<dyn PipelineSystem> = Arc::new(sys);
                runtime.resources.insert(sys);
            }
            Err(e) => log::error!("pipeline system init failed: {e}"),
        }

        // Physics — Rapier3D
        let physics: Box<dyn PhysicsProvider> = Box::new(RapierPhysicsWorld::default());
        runtime.backends.insert(Arc::new(Mutex::new(physics)));

        // UI layout — Taffy
        let layout: Box<dyn LayoutSystem> = Box::new(TaffyLayoutSystem::new());
        runtime.backends.insert(Arc::new(Mutex::new(layout)));

        // Text renderer — StandardTextRenderer
        let text: Arc<dyn TextRenderer> = Arc::new(StandardTextRenderer::new(TEXT_WGSL.to_owned()));
        runtime.backends.insert(text);

        // Audio — shared mix bus + CPAL device. The bus is the sole
        // synchronisation boundary between audio lanes (main thread) and
        // the backend's hardware callback (RT thread). The opened
        // AudioStream is stored as a backend handle; dropping it stops
        // the stream.
        let stream_info = StreamInfo {
            channels: 2,
            sample_rate: 48_000,
        };
        let mix_bus: Arc<dyn AudioMixBus> = Arc::new(DefaultMixBus::new(stream_info, 8192));
        runtime.resources.insert(Arc::clone(&mix_bus));
        let device: Box<dyn AudioDevice> = Box::new(CpalAudioDevice::new());
        match device.open(mix_bus) {
            Ok(stream) => {
                let stream: Arc<dyn khora_sdk::AudioStream> = Arc::from(stream);
                runtime.backends.insert(stream);
            }
            Err(e) => log::error!("audio open failed: {}", e),
        }

        // `.wgsl` hot-reload (engine-dev convenience). Watch the canonical
        // shader source tree so edits to lighting / shadow / material WGSL
        // recompose the affected modules and rebuild the cached pipelines in
        // place — no rebuild, no lane change (the `shader_hot_reload` data
        // system pumps the watcher each tick; lanes re-fetch pipelines by key).
        // The directory is resolved from this crate's compile-time location, so
        // it only exists when the sandbox runs from the source checkout; a
        // relocated binary skips hot-reload and serves the embedded shaders.
        let shader_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/khora-infra/src/graphics/shader/shaders");
        if let Ok(shader_dir) = shader_dir.canonicalize() {
            match khora_sdk::AssetWatcher::new(&shader_dir) {
                Ok(watcher) => {
                    runtime.resources.insert(Arc::new(watcher));
                    log::info!(
                        "sandbox: watching {} for shader hot-reload",
                        shader_dir.display()
                    );
                }
                Err(e) => log::warn!(
                    "sandbox: shader hot-reload disabled ({}): {:#}",
                    shader_dir.display(),
                    e
                ),
            }
        }
    })?;
    Ok(())
}

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

//! End-to-end frame integration tests for the read-only data path of a tick:
//!
//! ```text
//! World ──► Flow (Substrate Pass) ──► LaneBus ──► (Lane consumption)
//! ```
//!
//! These tests exercise the Flow / LaneBus / scheduler wiring **without a GPU
//! or any backend service**. The real domain Flows (`RenderFlow`, `ShadowFlow`,
//! `AudioFlow`) are read-only projectors: given an ECS `World` and a `Runtime`
//! they publish typed Views into a `LaneBus`, which is exactly what the engine
//! tick does in production. We build a `World`, spawn entities, run the
//! Substrate Pass via [`substrate::run_flows`], then assert on the published
//! views.
//!
//! Lives in `khora-agents/tests/` alongside `physics_agent_tests.rs` because
//! that crate already pulls `khora-control` as a dev-dependency (the only crate
//! exposing `substrate::run_flows` / `run_data_systems`) and is the established
//! home for full-stack integration tests. No GPU is touched: `RenderFlow`,
//! `ShadowFlow` and `AudioFlow` project from ECS data alone. `UiFlow` is *not*
//! tested here — its projection needs a surface size and a text renderer from
//! the `Runtime`, which cannot be stubbed cheaply without a real backend.

use khora_core::lane::LaneBus;
use khora_core::math::{Mat4, Quaternion, Vec3};
use khora_core::renderer::api::pipeline::PrimitiveTopology;
use khora_core::renderer::api::resource::BufferId;
use khora_core::renderer::api::scene::GpuMesh;
use khora_core::renderer::api::util::IndexFormat;
use khora_core::renderer::light::{DirectionalLight, LightType, PointLight};
use khora_core::Runtime;

use khora_data::ecs::{Camera, GlobalTransform, HandleComponent, Light, Parent, Transform, World};
use khora_data::flow::{AudioView, ShadowMatrices, ShadowView};
use khora_data::render::RenderWorld;

use khora_core::asset::{AssetHandle, AssetUUID};
use std::sync::Arc;

// ─── helpers ────────────────────────────────────────────────────────────

/// A `Runtime` with no backends/services — enough for the GPU-free Flows.
/// `RenderFlow` / `ShadowFlow` consult an optional `EditorViewportOverride`
/// resource; absent (as here) the editor fingerprint is `0` and `primary_view`
/// falls back to scene cameras only, which is what we want for these tests.
fn bare_runtime() -> Arc<Runtime> {
    Arc::new(Runtime::new())
}

/// A dummy resolved `GpuMesh` handle, standing in for a mesh that was already
/// uploaded — lets us populate `RenderWorld.meshes` without a real GPU device.
fn gpu_mesh_handle() -> HandleComponent<GpuMesh> {
    HandleComponent {
        handle: AssetHandle::new(GpuMesh {
            vertex_buffer: BufferId(0),
            index_buffer: BufferId(0),
            index_count: 0,
            index_format: IndexFormat::Uint32,
            primitive_topology: PrimitiveTopology::TriangleList,
        }),
        uuid: AssetUUID::new(),
    }
}

/// Spawns a renderable mesh entity (transform + global transform + GPU mesh
/// handle) at `position`. Returns its id.
fn spawn_mesh(world: &mut World, position: Vec3) -> khora_core::ecs::entity::EntityId {
    world.spawn((
        Transform::from_translation(position),
        GlobalTransform::at_position(position),
        gpu_mesh_handle(),
    ))
}

/// Runs every registered Flow over `world` and returns the published bus.
fn run_substrate(world: &mut World, runtime: &Runtime) -> LaneBus {
    let mut bus = LaneBus::new();
    khora_control::substrate::run_flows(world, &mut bus, runtime);
    bus
}

/// A `Runtime` carrying a `SharedTime` resource fixed at `alpha` plus an empty
/// `SharedTransformInterpolation` store, so the `RenderFlow`'s interpolation
/// path is exercised at a known factor. Use [`record_previous_pose`] to seed a
/// previous pose for an entity.
fn runtime_with_alpha(alpha: f32) -> Runtime {
    let mut runtime = Runtime::new();
    let time = khora_core::time::Time {
        interpolation_alpha: alpha,
        ..khora_core::time::Time::default()
    };
    let shared: khora_core::time::SharedTime = Arc::new(std::sync::RwLock::new(time));
    runtime.resources.insert(shared);

    let interp: khora_core::interpolation::SharedTransformInterpolation = Arc::new(
        std::sync::RwLock::new(khora_core::interpolation::TransformInterpolation::new()),
    );
    runtime.resources.insert(interp);
    runtime
}

/// Records a previous world-space pose for `entity` in the runtime's
/// interpolation store — the engine-owned analogue of "this body was here
/// before the last sim step".
fn record_previous_pose(
    runtime: &Runtime,
    entity: khora_core::ecs::entity::EntityId,
    pose: khora_core::math::affine_transform::AffineTransform,
) {
    let store = runtime
        .resources
        .get::<khora_core::interpolation::SharedTransformInterpolation>()
        .expect("runtime_with_alpha inserts the interpolation store");
    store.write().unwrap().record(entity, pose);
}

// ─── 1. Spawn → Flow → Bus ──────────────────────────────────────────────

/// Builds a World with a mesh, a light and an active camera, runs the
/// Substrate Pass, and asserts the published `RenderWorld` reflects all three:
/// one mesh, one light, and a camera view present. This is the core e2e
/// guarantee — entities spawned in the World surface in the bus view the lanes
/// consume.
#[test]
fn spawn_flow_bus_publishes_render_world() {
    let mut world = World::new();
    let runtime = bare_runtime();

    spawn_mesh(&mut world, Vec3::new(1.0, 2.0, 3.0));

    world.spawn((
        Light::new(LightType::Directional(DirectionalLight::default())),
        GlobalTransform::at_position(Vec3::new(0.0, 10.0, 0.0)),
    ));

    let cam_pos = Vec3::new(0.0, 0.0, 5.0);
    world.spawn((
        Camera::new_perspective(60.0_f32.to_radians(), 16.0 / 9.0, 0.1, 1000.0),
        GlobalTransform::at_position(cam_pos),
    ));

    let bus = run_substrate(&mut world, &runtime);

    let rw = bus
        .get::<RenderWorld>()
        .expect("RenderFlow must publish a RenderWorld into the bus");

    assert_eq!(rw.meshes.len(), 1, "one mesh should be extracted");
    assert_eq!(rw.lights.len(), 1, "one enabled light should be extracted");
    assert_eq!(
        rw.directional_light_count(),
        1,
        "the extracted light is directional"
    );
    assert_eq!(rw.views.len(), 1, "the active camera should yield one view");

    // The mesh world-space transform must round-trip through the projection.
    let m = &rw.meshes[0];
    assert_eq!(m.transform.translation(), Vec3::new(1.0, 2.0, 3.0));

    // The view position must match the camera's global translation.
    assert_eq!(rw.views[0].position, cam_pos);
}

/// A disabled light and an inactive camera must NOT appear in the view — the
/// projection respects the `enabled` / `is_active` flags, so the bus reflects
/// game-observable scene state, not raw component presence.
#[test]
fn disabled_light_and_inactive_camera_are_excluded() {
    let mut world = World::new();
    let runtime = bare_runtime();

    world.spawn((
        Light {
            light_type: LightType::Point(PointLight::default()),
            enabled: false,
        },
        GlobalTransform::default(),
    ));

    let mut inactive_cam = Camera::default_perspective();
    inactive_cam.is_active = false;
    world.spawn((inactive_cam, GlobalTransform::default()));

    let bus = run_substrate(&mut world, &runtime);
    let rw = bus.get::<RenderWorld>().expect("RenderWorld published");

    assert!(rw.lights.is_empty(), "disabled light must be excluded");
    assert!(
        rw.views.is_empty(),
        "inactive camera must not produce a view (no editor override present)"
    );
}

// ─── 2. Mutation across "frames" ────────────────────────────────────────

/// Runs the Substrate Pass, mutates the World (move a mesh, add a light),
/// runs it again, and asserts every change surfaces in the re-published view.
/// This exercises the per-domain epoch + Flow view-cache path: each mutation
/// bumps the relevant domain epoch, so the cache key changes and the Flow
/// re-projects rather than serving a stale view.
#[test]
fn mutation_across_frames_reprojects_not_stale() {
    let mut world = World::new();
    let runtime = bare_runtime();

    let mover = spawn_mesh(&mut world, Vec3::ZERO);
    spawn_mesh(&mut world, Vec3::new(5.0, 0.0, 0.0));

    // Frame 1.
    {
        let bus = run_substrate(&mut world, &runtime);
        let rw = bus.get::<RenderWorld>().expect("RenderWorld published");
        assert_eq!(rw.meshes.len(), 2, "two meshes in frame 1");
        assert!(rw.lights.is_empty(), "no lights in frame 1");
    }

    // Mutate: move one mesh (Spatial epoch bump) and add a light (Render epoch
    // bump). Both bumps must invalidate the frame-1 cached view.
    {
        let gt = world
            .get_mut::<GlobalTransform>(mover)
            .expect("mover has a GlobalTransform");
        *gt = GlobalTransform::at_position(Vec3::new(0.0, 7.0, 0.0));
    }
    world.spawn((
        Light::new(LightType::Point(PointLight::default())),
        GlobalTransform::at_position(Vec3::new(2.0, 2.0, 2.0)),
    ));

    // Frame 2 — the cache must NOT serve the stale frame-1 view.
    let bus = run_substrate(&mut world, &runtime);
    let rw = bus.get::<RenderWorld>().expect("RenderWorld published");

    assert_eq!(rw.meshes.len(), 2, "both meshes still present");
    let moved = rw
        .meshes
        .iter()
        .find(|m| m.transform.translation() == Vec3::new(0.0, 7.0, 0.0));
    assert!(
        moved.is_some(),
        "the moved mesh's new position must surface in the re-projected view (not stale)"
    );
    assert_eq!(rw.lights.len(), 1, "the added light appears in frame 2");
    assert_eq!(
        rw.point_light_count(),
        1,
        "the added light is a point light"
    );
}

/// Despawning an entity surfaces in the next frame's view. Uses a
/// single-component entity (one domain → one page) so the despawn exercises
/// only the epoch-bump + re-projection path the cache relies on; the moved
/// light disappears from `RenderWorld` after despawn.
#[test]
fn despawn_across_frames_reprojects() {
    let mut world = World::new();
    let runtime = bare_runtime();

    spawn_mesh(&mut world, Vec3::ZERO);
    let light = world.spawn((
        Light::new(LightType::Point(PointLight::default())),
        GlobalTransform::at_position(Vec3::new(1.0, 1.0, 1.0)),
    ));

    {
        let bus = run_substrate(&mut world, &runtime);
        let rw = bus.get::<RenderWorld>().expect("RenderWorld published");
        assert_eq!(rw.lights.len(), 1, "light present in frame 1");
    }

    assert!(world.despawn(light), "light should despawn");

    let bus = run_substrate(&mut world, &runtime);
    let rw = bus.get::<RenderWorld>().expect("RenderWorld published");
    assert!(
        rw.lights.is_empty(),
        "despawned light must be gone from the re-projected view"
    );
    assert_eq!(rw.meshes.len(), 1, "the mesh is unaffected by the despawn");
}

/// Despawning the FIRST of two multi-domain mesh entities must leave the
/// survivor fully projected. Each mesh spans Spatial (`Transform` +
/// `GlobalTransform`) and Render (`HandleComponent<GpuMesh>`) within one page,
/// so despawning the first row must `swap_remove` that physical row exactly
/// once — the survivor's Render data must remain reachable.
#[test]
fn despawn_first_of_two_meshes_keeps_survivor() {
    let mut world = World::new();
    let runtime = bare_runtime();

    let first = spawn_mesh(&mut world, Vec3::ZERO);
    spawn_mesh(&mut world, Vec3::new(5.0, 0.0, 0.0));

    {
        let bus = run_substrate(&mut world, &runtime);
        let rw = bus.get::<RenderWorld>().expect("RenderWorld published");
        assert_eq!(rw.meshes.len(), 2, "both meshes present in frame 1");
    }

    assert!(world.despawn(first), "first mesh should despawn");

    let bus = run_substrate(&mut world, &runtime);
    let rw = bus.get::<RenderWorld>().expect("RenderWorld published");
    assert_eq!(
        rw.meshes.len(),
        1,
        "the surviving mesh must still be projected after despawning the first"
    );
}

/// Re-running the Substrate Pass over an **unchanged** World republishes a view
/// consistent with the first frame. The cache key is stable across frames when
/// no domain epoch moved, so the cached view is reused — and that reused view
/// is still correct (same mesh/light/view counts), never empty or dropped.
#[test]
fn unchanged_world_republishes_consistent_view() {
    let mut world = World::new();
    let runtime = bare_runtime();

    spawn_mesh(&mut world, Vec3::new(1.0, 0.0, 0.0));
    spawn_mesh(&mut world, Vec3::new(2.0, 0.0, 0.0));
    world.spawn((
        Light::new(LightType::Directional(DirectionalLight::default())),
        GlobalTransform::default(),
    ));

    let (m1, l1) = {
        let bus = run_substrate(&mut world, &runtime);
        let rw = bus.get::<RenderWorld>().expect("RenderWorld published");
        (rw.meshes.len(), rw.lights.len())
    };

    // No mutation between the two passes.
    let bus = run_substrate(&mut world, &runtime);
    let rw = bus.get::<RenderWorld>().expect("RenderWorld published");

    assert_eq!(rw.meshes.len(), m1, "mesh count stable across frames");
    assert_eq!(rw.lights.len(), l1, "light count stable across frames");
    assert_eq!(m1, 2);
    assert_eq!(l1, 1);
}

// ─── 3. Multi-domain coherence ──────────────────────────────────────────

/// In a single Substrate Pass, `RenderFlow`, `ShadowFlow` and `AudioFlow` all
/// publish coherent views derived from the same World. A shadow-casting
/// directional light, observed through an active camera, must appear in both
/// `RenderWorld.lights` and `ShadowView.matrices`; the `AudioView` is present
/// and empty (no audio sources). This confirms the multi-Flow fan-out wiring.
#[test]
fn multi_domain_views_are_coherent() {
    let mut world = World::new();
    let runtime = bare_runtime();

    // Active camera — directional shadows need a camera frustum.
    world.spawn((
        Camera::new_perspective(60.0_f32.to_radians(), 16.0 / 9.0, 0.1, 1000.0),
        GlobalTransform::at_position(Vec3::new(0.0, 0.0, 10.0)),
    ));

    // A shadow-casting directional light.
    let dir = DirectionalLight {
        shadow_enabled: true,
        ..Default::default()
    };
    world.spawn((
        Light::new(LightType::Directional(dir)),
        GlobalTransform::at_position(Vec3::new(0.0, 20.0, 0.0)),
    ));

    spawn_mesh(&mut world, Vec3::ZERO);

    let bus = run_substrate(&mut world, &runtime);

    let rw = bus.get::<RenderWorld>().expect("RenderWorld published");
    let sv = bus.get::<ShadowView>().expect("ShadowView published");
    let av = bus.get::<AudioView>().expect("AudioView published");

    // Same light, two views.
    assert_eq!(rw.lights.len(), 1, "RenderWorld sees the light");
    assert_eq!(
        sv.light_count, 1,
        "ShadowView counts the same enabled light"
    );

    // Light index 0 (first enabled light) casts a single (directional) matrix.
    match sv.matrices.get(&0) {
        Some(ShadowMatrices::Single(m)) => {
            assert_ne!(
                *m,
                Mat4::IDENTITY,
                "a fitted directional shadow matrix is not identity"
            );
        }
        other => panic!("expected a Single shadow matrix at light 0, got {other:?}"),
    }

    // Audio domain is wired even with nothing to play.
    assert_eq!(av.source_count, 0, "no audio sources spawned");
    assert!(av.sources.is_empty());
}

// ─── 4. Transform propagation feeds the flows ───────────────────────────

/// Spawns a parent + child hierarchy, runs the `transform_propagation`
/// DataSystem (the `PostSimulation`-phase maintenance step that composes
/// `Transform` into `GlobalTransform`), then runs the Substrate Pass. The
/// child's `GlobalTransform` must compose the parent's, and the projected
/// `RenderWorld` mesh for the child must carry that composed world-space
/// position — confirming the data path produces correct world-space data
/// before the flows read it.
#[test]
fn transform_propagation_feeds_render_flow() {
    use khora_control::substrate;
    use khora_core::lane::OutputDeck;
    use khora_data::ecs::TickPhase;

    let mut world = World::new();
    let runtime = bare_runtime();

    // Parent at (10, 0, 0). GlobalTransform starts at identity; propagation
    // overwrites it from the local Transform.
    let parent = world.spawn((
        Transform::from_translation(Vec3::new(10.0, 0.0, 0.0)),
        GlobalTransform::identity(),
    ));

    // Child offset (0, 2, 0) in parent space, carrying a renderable mesh.
    let child = world.spawn((
        Transform::from_translation(Vec3::new(0.0, 2.0, 0.0)),
        GlobalTransform::identity(),
        Parent(parent),
        gpu_mesh_handle(),
    ));

    // Run the propagation DataSystem through the real substrate entry point.
    let mut deck = OutputDeck::new();
    substrate::run_data_systems(&mut world, &runtime, &mut deck, TickPhase::PostSimulation);

    // The child's world position composes parent ∘ child = (10, 2, 0).
    let child_gt = world
        .get::<GlobalTransform>(child)
        .expect("child has a GlobalTransform");
    let expected = Vec3::new(10.0, 2.0, 0.0);
    let got = child_gt.0.translation();
    assert!(
        (got - expected).length() < 1e-4,
        "child global translation should compose the parent's: expected {expected:?}, got {got:?}"
    );

    // The flow then reads that composed GlobalTransform.
    let bus = run_substrate(&mut world, &runtime);
    let rw = bus.get::<RenderWorld>().expect("RenderWorld published");

    assert_eq!(rw.meshes.len(), 1, "the child mesh is extracted");
    let got = rw.meshes[0].transform.translation();
    assert!(
        (got - expected).length() < 1e-4,
        "the projected mesh carries the world-space position: expected {expected:?}, got {got:?}"
    );

    // Sanity: the rotation-free composition is a pure translation.
    let _ = Quaternion::IDENTITY;
}

// ─── 5. Render interpolation ────────────────────────────────────────────

/// An entity with a recorded previous pose and a current `GlobalTransform` must
/// be projected at the blended world-space position for the active alpha. Here
/// previous=(0,0,0), current=(10,0,0), alpha=0.5 ⇒ the projected mesh sits at
/// (5,0,0) — render-only interpolation, the authoritative transforms untouched.
#[test]
fn render_flow_interpolates_when_previous_present() {
    let mut world = World::new();
    let runtime = runtime_with_alpha(0.5);

    let entity = world.spawn((
        Transform::from_translation(Vec3::new(10.0, 0.0, 0.0)),
        GlobalTransform::at_position(Vec3::new(10.0, 0.0, 0.0)),
        gpu_mesh_handle(),
    ));
    record_previous_pose(&runtime, entity, GlobalTransform::at_position(Vec3::ZERO).0);

    let bus = run_substrate(&mut world, &runtime);
    let rw = bus.get::<RenderWorld>().expect("RenderWorld published");

    assert_eq!(rw.meshes.len(), 1);
    let got = rw.meshes[0].transform.translation();
    let expected = Vec3::new(5.0, 0.0, 0.0);
    assert!(
        (got - expected).length() < 1e-4,
        "interpolated mesh should sit at the alpha-blended position: expected {expected:?}, got {got:?}"
    );

    // The authoritative GlobalTransform is untouched — only the projection blends.
    let gt = world.get::<GlobalTransform>(entity).unwrap();
    assert_eq!(gt.0.translation(), Vec3::new(10.0, 0.0, 0.0));
}

/// alpha=1.0 with a previous pose must yield exactly the current world-space
/// position (the endpoint of the blend).
#[test]
fn render_flow_alpha_one_yields_current_transform() {
    let mut world = World::new();
    let runtime = runtime_with_alpha(1.0);

    let entity = world.spawn((
        Transform::from_translation(Vec3::new(4.0, 0.0, 0.0)),
        GlobalTransform::at_position(Vec3::new(4.0, 0.0, 0.0)),
        gpu_mesh_handle(),
    ));
    record_previous_pose(&runtime, entity, GlobalTransform::at_position(Vec3::ZERO).0);

    let bus = run_substrate(&mut world, &runtime);
    let rw = bus.get::<RenderWorld>().expect("RenderWorld published");

    let got = rw.meshes[0].transform.translation();
    assert!((got - Vec3::new(4.0, 0.0, 0.0)).length() < 1e-4);
}

/// An entity with **no** recorded previous pose is projected at its current
/// transform unchanged, regardless of the active alpha — interpolation only
/// applies to entities the sim moves.
#[test]
fn render_flow_without_previous_uses_current_transform() {
    let mut world = World::new();
    let runtime = runtime_with_alpha(0.5);

    spawn_mesh(&mut world, Vec3::new(7.0, 0.0, 0.0));

    let bus = run_substrate(&mut world, &runtime);
    let rw = bus.get::<RenderWorld>().expect("RenderWorld published");

    let got = rw.meshes[0].transform.translation();
    assert!(
        (got - Vec3::new(7.0, 0.0, 0.0)).length() < 1e-4,
        "an entity without a previous transform must render at its current position, got {got:?}"
    );
}

# Physics

Rigid-body simulation through the `PhysicsProvider` trait. Default backend is Rapier3D.

- Document — Khora Physics v1.0
- Status — Authoritative
- Date — May 2026

---

## Contents

1. The contract
2. Pipeline
3. Components
4. Fixed timestep
5. The default backend — Rapier3D
6. PhysicsAgent and GORNA
7. For game developers
8. For engine contributors
9. Decisions
10. Open questions

---

## 01 — The contract

The physics surface is a single trait in `khora-core`:

```rust
pub trait PhysicsProvider: Send + Sync {
    fn step(&mut self, dt: f32);
    fn add_rigid_body(&mut self, ...) -> RigidBodyHandle;
    fn add_collider(&mut self, ...) -> ColliderHandle;
    fn raycast(&self, origin: Vec3, dir: Vec3, max_dist: f32) -> Option<RaycastHit>;
    // ...
}
```

`PhysicsAgent` does not call Rapier. It calls `PhysicsProvider`. The default implementation today is the Rapier3D backend in `khora-infra`. A future native Khora solver (see [Roadmap](./roadmap.md) Phase 6) drops in as a new implementation of the same trait without touching agent or lane code.

## 02 — Pipeline

```
ECS (RigidBody, Collider, GlobalTransform)
  ↓ sync to PhysicsProvider
StandardPhysicsLane::execute()
  ↓ PhysicsProvider::step(dt)
  ↓ sync back to ECS (updated positions/rotations)
PhysicsDebugLane (optional: visualize collision shapes)
```

The `StandardPhysicsLane` is the only required lane. `PhysicsDebugLane` is opt-in, switched on through the editor for debugging.

## 03 — Components

| Component | Purpose |
|---|---|
| `Transform` | Local pose — physics reads it on body creation, writes back after `step` |
| `GlobalTransform` | World-space pose — synced from physics every frame |
| `RigidBody` | Body type (Dynamic, Static, Kinematic), mass, velocity, CCD flag |
| `Collider` | Shape descriptor — Cuboid, Sphere, Capsule, TriMesh, ConvexHull |

`RigidBody::Dynamic` participates in dynamics. `Static` is unmovable terrain. `Kinematic` is moved by code, not by forces, but pushes other bodies.

Continuous Collision Detection (CCD) is opt-in per body via `RigidBody::with_ccd(true)`. It catches tunneling at the cost of step time; use it for fast-moving small bodies (bullets, thrown objects).

## 04 — Fixed timestep

`PhysicsAgent` uses a fixed timestep with an accumulator pattern:

```rust
self.accumulator += dt;
while self.accumulator >= self.fixed_step {
    self.provider.step(self.fixed_step);
    self.accumulator -= self.fixed_step;
}
```

Default: `fixed_step = 1.0 / 60.0`. GORNA may negotiate this — under heavy load, `PhysicsAgent` can switch to the Simplified strategy with a longer step or, in extremis, to Disabled (no simulation).

Determinism is the reason for fixed timestep. Variable steps cause subtle simulation drift across machines and replays.

## 05 — The default backend — Rapier3D

| File | Purpose |
|---|---|
| `crates/khora-infra/src/physics/rapier/mod.rs` | `RapierPhysicsProvider` — implements `PhysicsProvider` |

Rapier3D 0.x is the dependency. The wrapper translates Khora's `RigidBody` / `Collider` / `Vec3` / `Quat` into Rapier types and back. Raycasts go through Rapier's `QueryPipeline`.

Future: a native Khora solver replaces Rapier without touching `StandardPhysicsLane` or `PhysicsAgent`. The roadmap targets MLS-MPM for unified simulation, IPC for collision, XPBD + ADMM for constraints. See [Roadmap](./roadmap.md) Phase 6.

## 06 — PhysicsAgent and GORNA

`PhysicsAgent` exposes three strategies:

| Strategy | Lane | When |
|---|---|---|
| **Standard** | `StandardPhysicsLane`, fixed_step = 1/60 s | Healthy budget, normal scene |
| **Simplified** | `StandardPhysicsLane`, fixed_step = 1/30 s | Mid-pressure — half the simulation cost |
| **Disabled** | None | Death spiral — physics turned off until recovery |

GORNA picks based on frame budget, GPU pressure (which can crowd CPU through synchronization), and death-spiral detection. The transition is graceful — bodies keep their state; only the step rate changes.

---

## For game developers

```rust
// A static ground plane
world.spawn((
    Transform::default(),
    GlobalTransform::identity(),
    RigidBody::static_(),
    Collider::cuboid(Vec3::new(50.0, 0.1, 50.0)),
));

// A dynamic falling box
world.spawn((
    Transform::from_translation(Vec3::new(0.0, 5.0, 0.0)),
    GlobalTransform::identity(),
    RigidBody::dynamic().with_mass(1.0),
    Collider::cuboid(Vec3::new(0.5, 0.5, 0.5)),
));

// A bullet — fast, small, CCD on
world.spawn((
    Transform::from_translation(player_pos),
    GlobalTransform::identity(),
    RigidBody::dynamic().with_velocity(forward * 50.0).with_ccd(true),
    Collider::sphere(0.05),
));
```

For raycasts and shape queries from gameplay code, use `PhysicsQueryService` from `khora-agents` — an on-demand wrapper that does not require an active `PhysicsAgent` step:

```rust
let service = ctx.services.get::<Arc<PhysicsQueryService>>().unwrap();
if let Some(hit) = service.raycast(origin, dir, 100.0) {
    log::info!("Hit entity {:?} at {:?}", hit.entity, hit.point);
}
```

## For engine contributors

The split is clean:

| File | Purpose |
|---|---|
| `crates/khora-core/src/physics/` | `PhysicsProvider` trait, body and collider types, raycast types |
| `crates/khora-lanes/src/physics_lane/standard.rs` | `StandardPhysicsLane` — calls `PhysicsProvider::step` |
| `crates/khora-lanes/src/physics_lane/debug.rs` | `PhysicsDebugLane` — visualization |
| `crates/khora-agents/src/physics_agent/mod.rs` | `PhysicsAgent` — accumulator, GORNA negotiation |
| `crates/khora-infra/src/physics/rapier/` | Rapier3D backend |

To add a backend: implement `PhysicsProvider` in a new `khora-infra/src/physics/<backend>/` folder, register it as a service in the SDK init. Done. The agent and lane are unchanged.

To add a new strategy: today there are three (Standard / Simplified / Disabled). A fourth would be added to `PhysicsAgent::negotiate` with a different `StrategyOption` — for example, a SIMD-accelerated path enabled on supported CPUs.

## Decisions

### We said yes to
- **`PhysicsProvider` trait, single contract.** Everything physics-related goes through it. No agent or lane reaches into Rapier directly.
- **Fixed timestep with accumulator.** Determinism beats per-frame variance.
- **CCD as opt-in per body.** Free for static and slow-moving objects; available where it matters.
- **Strategy includes Disabled.** Better to render a frame with no physics than to drop a frame.

### We said no to
- **Calling Rapier from agents or game code.** The seam is `PhysicsProvider`. Anything else couples the engine to one backend.
- **A separate physics tick loop.** PhysicsAgent owns its accumulator. The frame loop is one loop.
- **Variable timestep.** Considered. Rejected. Determinism is load-bearing for replays, multiplayer, and bug reports.

## Open questions

1. **Per-region simulation rate.** "Use Standard near the player, Simplified everywhere else" is a stated goal of AGDF — the API for it is not built.
2. **Physics state in serialization.** `SerializationGoal::FastestLoad` does not preserve velocities or contacts (see [Serialization](./14_serialization.md)). Whether to add a "snapshot with physics" goal is open.
3. **Native solver migration.** Roadmap Phase 6. The trait surface is stable enough; the implementation is a multi-quarter effort.

---

*Next: spatial audio. See [Audio](./11_audio.md).*

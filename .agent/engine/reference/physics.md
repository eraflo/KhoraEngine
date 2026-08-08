# Physics — reference

Domain knowledge for Khora physics. Consulted during Research (dispatch the `codebase-*` subagents to
apply it to concrete files). Follow [`../RULES.md`](../RULES.md) §6 (subsystem boundaries).

## Scope
Rigid-body dynamics, colliders and shapes, joints/constraints, CCD, broadphase/solver tuning,
ECS↔physics synchronization.

## Key files
- Trait: `crates/khora-core/src/physics/` (`PhysicsProvider`, `BodyType`, `ColliderShape`).
- Backends, one subfolder each: `crates/khora-infra/src/physics/rapier/` (`RapierPhysicsWorld`,
  `conversions.rs`) — the one the engine runs; and `crates/khora-infra/src/physics/khora/` — an
  in-house broad phase, narrow phase and impulse solver that **does not implement `PhysicsProvider`
  yet** and is wired to nothing. Deliberate future work, not abandoned code.
- Lanes: `crates/khora-lanes/src/physics_lane/` (Standard, CCD).
- Components / flow: `crates/khora-data/src/ecs/components/physics/`, `crates/khora-data/src/flow/physics.rs`.

## Hard rules
- Route physics through `PhysicsProvider` + the physics lane. **Never** call Rapier directly from agents.
- The `PhysicsAgent` stays a strategist (Standard / Simplified) — no per-frame state.
- Distance-based detach/reattach of `RigidBody` is **semantic** (changes the simulation) → it is an opt-in,
  developer-authored `DataSystem`, never automatic AGDF.
- Convert math at the boundary via `conversions.rs`; engine-side math is `khora_core::math`.

## Who writes a pose

`ComponentProvenance` decides, and three components split what one used to hold:

- `Transform` (`Authored`) — what a human, a tool or game code declared. Serialized.
- `SimulatedTransform` (`Runtime`) — the **world** pose the provider reports. Never serialized, never
  composed with a parent. `transform_propagation` prefers it unless the entity is `Teleported`.
- `BodyMotion` (`Runtime`) — the velocity the body *has*, as opposed to `RigidBody::initial_velocity`
  (`Authored`), which is applied once at creation.

Moving something is **declared**, not inferred: whoever moves an entity adds the `Teleported` marker,
and the sync consumes and removes it. The old 1 cm / 1.62° heuristic could not work — drift, a parent
frame, a sub-step overshoot and a real teleport all produce "the two values differ".

## Skills
- [`add-a-lane`](../skills/add-a-lane/SKILL.md) — add a physics lane / strategy.
- [`add-a-component`](../skills/add-a-component/SKILL.md) — add a physics component.
- [`debug-frame`](../skills/debug-frame/SKILL.md) · [`run-the-engine`](../skills/run-the-engine/SKILL.md) · [`build-and-test`](../skills/build-and-test/SKILL.md).

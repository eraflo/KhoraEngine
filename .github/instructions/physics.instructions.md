---
applyTo: "crates/khora-infra/src/physics/**,crates/khora-lanes/src/physics_lane/**"
---

# Physics — reference

Domain knowledge for Khora physics. Consulted during Research (dispatch the `codebase-*` subagents to
apply it to concrete files). Follow [`../RULES.md`](../RULES.md) §6 (subsystem boundaries).

## Scope
Rigid-body dynamics, colliders and shapes, joints/constraints, CCD, broadphase/solver tuning,
ECS↔physics synchronization.

## Key files
- Trait: `crates/khora-core/src/physics/` (`PhysicsProvider`, `BodyType`, `ColliderShape`).
- Backend: `crates/khora-infra/src/physics/rapier/` (`RapierPhysicsWorld`, `conversions.rs`).
- Lanes: `crates/khora-lanes/src/physics_lane/` (Standard, CCD).
- Components / flow: `crates/khora-data/src/ecs/components/physics/`, `crates/khora-data/src/flow/physics.rs`.

## Hard rules
- Route physics through `PhysicsProvider` + the physics lane. **Never** call Rapier directly from agents.
- The `PhysicsAgent` stays a strategist (Standard / Simplified) — no per-frame state.
- Distance-based detach/reattach of `RigidBody` is **semantic** (changes the simulation) → it is an opt-in,
  developer-authored `DataSystem`, never automatic AGDF.
- Convert math at the boundary via `conversions.rs`; engine-side math is `khora_core::math`.

Note: `physics_lane` currently queries the World directly (`native_lanes.rs`) — migrating it to an
`OutputDeck` writeback channel is open work (see [`../knowledge/MEMORY.md`](../knowledge/MEMORY.md)). Use
codegraph to map the sync path before changing it.

## Skills
- [`add-a-lane`](../skills/add-a-lane/SKILL.md) — add a physics lane / strategy.
- [`add-a-component`](../skills/add-a-component/SKILL.md) — add a physics component.
- [`debug-frame`](../skills/debug-frame/SKILL.md) · [`run-the-engine`](../skills/run-the-engine/SKILL.md) · [`build-and-test`](../skills/build-and-test/SKILL.md).

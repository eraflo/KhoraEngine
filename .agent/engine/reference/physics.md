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

A collider attached to a body is described **in its body's frame**: `ColliderDesc.position/rotation`
is the offset from `parent_body` (zero for a collider on the body's own entity), the world pose only
for a collider with no body. The Rapier backend writes it with `set_position_wrt_parent`, never
`set_position`, which would overwrite the world pose the solver derives from the body. Sending a world
pose for an attached collider put each shape at twice its distance from the origin, and a stack of
boxes blew apart. Rotations are read with `AffineTransform::rotation()`, which strips scale;
`Quaternion::from_rotation_matrix` wants an orthonormal basis. A simulated entity keeps its authored
scale: `transform_propagation` sizes the simulated pose as the hierarchy would.

A released entity is handed back **where it is**. An entity is simulated while it has a `RigidBody`,
or a `KinematicCharacterController` with a `Collider`; once neither holds, the `physics_release`
DataSystem (`PostSimulation`, after `transform_propagation`) rewrites its `Transform` to the pose it is
drawn at (in its parent's frame, the mirror where its author put it) and removes `SimulatedTransform`
and `BodyMotion`. Under a parent scaled unevenly, a child turned against the parent's axes keeps its
place and turn but takes the sheared shape any such child has — no `Transform` can express the
unsheared one. This is
the one write of `Transform` the physics makes — once, at a release an author caused. Snapping back to
the authored pose would send every crate a stash/re-attach system detaches back to its spawn; keeping
the simulated pose left released entities frozen, deaf to their `Transform` and their parent. A
character moves on from its `GlobalTransform` (what its collider was synced to), so a declared move of
a character sticks.

## What a `Collider` sends

- **Surface.** `Collider.friction` / `restitution` are the surface; there is no separate material
  component (a shared material *asset* would be a feature of its own).
- **Shape in local units.** `Collider.shape` is scaled by the entity's world scale before it reaches
  the provider (`ColliderDesc.shape` is world-size): a box per axis, a sphere by its largest axis, a
  capsule's half-height by y and radius by max(x, z). The backend replaces the shape when it differs,
  so a rescale or an edited shape takes effect next frame.
- **Sensor.** `Collider.is_sensor` reports `Started`/`Stopped` on the collision channel — always, with
  or without `ActiveEvents` — and never pushes: Rapier sensor, density 0 (adds no mass), and the
  character controller's query excludes sensors. A sensor also sees kinematic bodies, but **not** fixed
  pairs: a trigger placed against the floor or overlapping another trigger reports nothing at spawn.
- **Character.** `ColliderDesc.is_character` marks the collider of a `KinematicCharacterController`
  with no body. Rapier counts a bodiless collider as fixed, like the level, so the character's collider
  asks for fixed pairs itself — that is how a trigger sees a character.
- **Sleep.** The backend compares a collider's state through a shared borrow and takes it mutably only
  when something differs: in Rapier, `get_mut` alone marks a collider modified and keeps its body
  awake, so a per-frame sync that always took it mutably meant nothing ever slept. Rapier flags no
  change for a new friction or restitution, so the backend wakes the collider's body and its contact
  partners itself.
- **Resized in contact.** Rapier 0.32 keeps a pair's contact points across `set_shape` (it resets the
  pair's workspace, not its manifolds), so a body resting on the ground and grown in place stayed sunk.
  The backend replaces a collider that holds contact points instead of resizing it: the engine handle
  keeps naming it (an alias table in `RapierPhysicsWorld`), and the step that re-finds its contacts
  drops the `Stopped`/`Started` the swap raises with what it still touches. Rapier-specific; the
  in-house physics engine will not need it.

Moving something is **declared**, not inferred: whoever moves an entity adds the `Teleported` marker,
and the sync consumes and removes it. The old 1 cm / 1.62° heuristic could not work — drift, a parent
frame, a sub-step overshoot and a real teleport all produce "the two values differ".

## Skills
- [`add-a-lane`](../skills/add-a-lane/SKILL.md) — add a physics lane / strategy.
- [`add-a-component`](../skills/add-a-component/SKILL.md) — add a physics component.
- [`debug-frame`](../skills/debug-frame/SKILL.md) · [`run-the-engine`](../skills/run-the-engine/SKILL.md) · [`build-and-test`](../skills/build-and-test/SKILL.md).

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

//! What a script does to the world.
//!
//! Apart from [`builtins`](super::builtins), which compute and report, these are
//! the functions that *change* something — and none of them changes anything.
//! Each queues a [`WorldCommand`] into the frame's buffer, which the engine
//! applies at the boundary where mutation is legal.
//!
//! That is not a workaround for `RULES.md` §3. It is what lets the scripting
//! agent declare `AgentAccess::Isolated` and run beside the others on the worker
//! pool: a lane holding a `&mut World` never could.
//!
//! The engine types these take — `Vec3` and the rest — are declared in
//! [`engine_types`](super::engine_types), constructors and all.
//!
//! # Reading is narrower than writing, on purpose
//!
//! A script may write any entity it has a handle to, and may read only its own
//! position — because a write is a *request*, resolved later against the real
//! `World`, while a read has to be answered now from what the frame projected.
//! Widening the read side means projecting more into the view for every entity
//! whether or not a script asks. When something needs it, that is the change to
//! make; guessing at it now would cost every frame for a feature nobody has
//! written yet.

use khora_core::ecs::entity::EntityId;
use khora_core::math::Vec3;
use khora_core::script::WorldCommand;
use khora_macros::ergon_fn;

use super::{NativeContext, NativeError};

// ─── Reading ────────────────────────────────────────────────────────────────

/// Where this entity is, as the frame projected it.
///
/// The value the frame *started* with. A `Translate` queued a moment ago has not
/// been applied yet and is not reflected here — commands are performed at the
/// boundary, so within one turn a behavior sees the world it was handed. Reading
/// back a write inside the same frame would need the boundary to run mid-lane,
/// which is the thing the whole design is arranged to avoid.
#[ergon_fn]
fn position(context: &mut NativeContext<'_>) -> Result<Vec3, NativeError> {
    context.position.ok_or_else(|| {
        NativeError::new(
            "`Position` needs an entity the frame placed — a free function has none, \
             and neither does an entity with no transform",
        )
    })
}

// ─── Moving ─────────────────────────────────────────────────────────────────

/// Moves the entity, relative to where it already is.
///
/// Distinct from [`set_position`] at the boundary, not just here: two behaviors
/// nudging the same entity **compose**, so neither loses a write, while two
/// placing it conflict and one of the two is discarded.
#[ergon_fn]
fn translate(context: &mut NativeContext<'_>, entity: EntityId, delta: Vec3) {
    context
        .commands
        .push(WorldCommand::Translate { entity, delta });
}

/// Places the entity, replacing where it was.
#[ergon_fn]
fn set_position(context: &mut NativeContext<'_>, entity: EntityId, value: Vec3) {
    context
        .commands
        .push(WorldCommand::SetTranslation { entity, value });
}

/// Resizes the entity.
#[ergon_fn]
fn set_scale(context: &mut NativeContext<'_>, entity: EntityId, value: Vec3) {
    context
        .commands
        .push(WorldCommand::SetScale { entity, value });
}

// ─── Existing ───────────────────────────────────────────────────────────────

/// Destroys the entity, and everything parented to it.
///
/// The subtree goes because a child of a destroyed entity has nowhere to be:
/// leaving it would produce an entity whose parent no longer exists, which is a
/// state nothing else in the engine knows how to render or simulate.
///
/// A behavior that despawns itself still finishes the member it is in, and its
/// `OnDespawn` runs before the frame ends — the entity is doomed from here, not
/// gone.
#[ergon_fn]
fn despawn(context: &mut NativeContext<'_>, entity: EntityId) {
    context.commands.push(WorldCommand::Despawn { entity });
}

// ─── Hierarchy ──────────────────────────────────────────────────────────────

/// Attaches the entity under a new parent.
///
/// The engine refuses a cycle at the boundary rather than here: whether `parent`
/// is already below `entity` depends on the `World`, which a lane cannot ask.
/// The command is diagnosed and dropped, and the rest of the frame's work
/// continues.
#[ergon_fn]
fn set_parent(context: &mut NativeContext<'_>, entity: EntityId, parent: EntityId) {
    context.commands.push(WorldCommand::SetParent {
        entity,
        parent: Some(parent),
    });
}

/// Detaches the entity from whatever it was parented to.
///
/// Its own function rather than `SetParent(e, null)`: an optional `Entity`
/// argument would make every hierarchy call carry a nullable the caller has to
/// think about, to express something that is a different intent anyway.
#[ergon_fn]
fn detach(context: &mut NativeContext<'_>, entity: EntityId) {
    context.commands.push(WorldCommand::SetParent {
        entity,
        parent: None,
    });
}

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

//! What a script does to the world: each native queues a [`WorldCommand`],
//! applied at the frame's boundary, never at the call.
//! See the book: concepts/scripting/writing.md, "A script never writes the world".

use khora_core::ecs::entity::EntityId;
use khora_core::math::{Quaternion, Vec3};
use khora_core::script::WorldCommand;
use khora_macros::ergon_fn;

use super::{NativeContext, NativeError};

// ─── Reading ────────────────────────────────────────────────────────────────

/// The natives reading the entity a behavior is attached to — refused in a
/// system, which has none.
pub const SUBJECT_NATIVES: &[&str] = &["Position"];

/// Where this entity is, as the frame projected it.
///
/// The value the frame started with: a `Translate` queued in the same turn is
/// not reflected until the boundary applies it.
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
/// Two behaviors nudging the same entity compose at the boundary, while two
/// [`set_position`]s conflict and one is discarded.
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

/// Turns the entity, replacing its rotation.
#[ergon_fn]
fn set_rotation(context: &mut NativeContext<'_>, entity: EntityId, value: Quaternion) {
    context
        .commands
        .push(WorldCommand::SetRotation { entity, value });
}

/// The component placed by its own operations rather than written, with them.
///
/// They allocate nothing and look nothing up; a script writing `Transform` is
/// pointed at them.
pub const PLACEMENT: (&str, &[&str]) = (
    "Transform",
    &["SetPosition", "Translate", "SetRotation", "SetScale"],
);

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
/// A child of a destroyed entity would have no parent, so the subtree goes. A
/// behavior that despawns itself still finishes the member it is in, and its
/// `OnDespawn` runs before the frame ends.
#[ergon_fn]
fn despawn(context: &mut NativeContext<'_>, entity: EntityId) {
    context.commands.push(WorldCommand::Despawn { entity });
}

// ─── Hierarchy ──────────────────────────────────────────────────────────────

/// Attaches the entity under a new parent.
///
/// A cycle is refused at the boundary, where the `World` can tell: the command
/// is diagnosed and dropped, and the frame's other work continues.
#[ergon_fn]
fn set_parent(context: &mut NativeContext<'_>, entity: EntityId, parent: EntityId) {
    context.commands.push(WorldCommand::SetParent {
        entity,
        parent: Some(parent),
    });
}

/// Detaches the entity from whatever it was parented to.
///
/// Its own function rather than `e.SetParent(null)`, so no hierarchy call
/// carries a nullable parent.
#[ergon_fn]
fn detach(context: &mut NativeContext<'_>, entity: EntityId) {
    context.commands.push(WorldCommand::SetParent {
        entity,
        parent: None,
    });
}

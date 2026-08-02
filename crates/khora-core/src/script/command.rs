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

//! What a script asks the engine to do.
//!
//! A script spawns, moves and destroys — all of which are structural or mutating
//! writes to the `World`, and all of which `RULES.md` §3 forbids inside a Lane.
//! Rather than perforate that rule, a script does not write at all: it *describes*
//! the write, and the engine performs it at the frame boundary where mutation is
//! already legal.
//!
//! The rule turns out to be the design. A lane that touches nothing shared can
//! declare [`AgentAccess::Isolated`] and run on the worker pool alongside the
//! others — which a lane holding a `&mut World` never could.
//!
//! # Two tiers, on purpose
//!
//! Transform writes are the overwhelming majority of what gameplay does, so they
//! are their own variants: typed, `Copy`, no lookup and no allocation. Everything
//! else goes through [`WorldCommand::SetComponent`] and friends, which carry a
//! [`ComponentName`] the applier resolves. The generic path costs a string and a
//! map hit; the common path costs neither.
//!
//! # Scope
//!
//! ECS mutations only. A script that plays a sound or applies an impulse is not
//! writing to the `World`, and routing those through here would make this enum a
//! dumping ground for every subsystem. They belong to their own decks, read by
//! the subsystem that owns them.
//!
//! [`AgentAccess::Isolated`]: crate::agent::AgentAccess::Isolated

use crate::ecs::entity::EntityId;
use crate::math::{Quaternion, Vec3};

use super::ScriptValue;

/// The name of a component type, as a script spells it.
///
/// A newtype rather than a bare `String` so the representation can become an
/// interned id later without touching a single call site. Today the generic
/// component path is the cold path, and a string is what the applier needs to
/// look the type up.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ComponentName(String);

impl ComponentName {
    /// Names a component type.
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// The name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ComponentName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for ComponentName {
    fn from(name: &str) -> Self {
        Self::new(name)
    }
}

/// An effect a script asks the engine to apply.
#[derive(Debug, Clone, PartialEq)]
pub enum WorldCommand {
    /// Places the entity, replacing its translation.
    SetTranslation {
        /// The entity to move.
        entity: EntityId,
        /// The new translation.
        value: Vec3,
    },
    /// Moves the entity by `delta`, relative to where it already is.
    ///
    /// Distinct from [`Self::SetTranslation`] for a reason that matters at the
    /// boundary: two scripts nudging the same entity **compose**, and reporting
    /// that as a conflict would flag correct code. Two scripts *placing* it do
    /// conflict, because one of the two placements is silently lost.
    Translate {
        /// The entity to move.
        entity: EntityId,
        /// How far, in world space.
        delta: Vec3,
    },
    /// Replaces the entity's rotation.
    SetRotation {
        /// The entity to turn.
        entity: EntityId,
        /// The new rotation.
        value: Quaternion,
    },
    /// Replaces the entity's scale.
    SetScale {
        /// The entity to resize.
        entity: EntityId,
        /// The new scale.
        value: Vec3,
    },
    /// Re-parents the entity, or detaches it when `parent` is `None`.
    SetParent {
        /// The child.
        entity: EntityId,
        /// The new parent, or `None` to detach.
        parent: Option<EntityId>,
    },
    /// Creates an entity carrying the given components.
    ///
    /// The components are named rather than the *prototype* that produced them:
    /// resolving `Spawn<Explosion>` into a set of components is the compiler's
    /// job, on the script side, where the prototype's declaration is in scope.
    /// Naming the prototype here would push that lookup into the frame boundary
    /// and make this command unapplicable until an asset path exists to resolve
    /// it against.
    ///
    /// The command carries no way to refer to the entity it creates, and that is
    /// deliberate: the id does not exist until the boundary applies this, so a
    /// reference would have to be a promise. The new entity configures itself in
    /// its own `OnSpawn`, which runs where the id is real — deferred creation is
    /// livable precisely because that hook exists.
    Spawn {
        /// Where to place it.
        ///
        /// Typed rather than left to `components` because a spawn always places
        /// something, and the spawner is what knows where: a projectile's
        /// heading belongs to whoever fired it.
        position: Vec3,
        /// How to orient it.
        rotation: Quaternion,
        /// Everything else it should carry.
        components: Vec<(ComponentName, ScriptValue)>,
    },
    /// Destroys the entity.
    Despawn {
        /// The entity to destroy.
        entity: EntityId,
    },
    /// Overwrites a component the entity already has.
    SetComponent {
        /// The entity.
        entity: EntityId,
        /// The component type.
        component: ComponentName,
        /// The new value.
        value: ScriptValue,
    },
    /// Attaches a component.
    AddComponent {
        /// The entity.
        entity: EntityId,
        /// The component type.
        component: ComponentName,
        /// Its initial value.
        value: ScriptValue,
    },
    /// Detaches a component.
    RemoveComponent {
        /// The entity.
        entity: EntityId,
        /// The component type.
        component: ComponentName,
    },
}

/// What an absolute write lands on.
///
/// Only what one command can *silently overwrite* for another. Accumulating
/// commands have no target because they cannot lose a write.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WriteTarget {
    /// The entity's translation.
    Translation,
    /// The entity's rotation.
    Rotation,
    /// The entity's scale.
    Scale,
    /// The entity's parent.
    Parent,
    /// Whether the entity exists at all.
    Existence,
    /// One component's value.
    Component(ComponentName),
}

impl std::fmt::Display for WriteTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Translation => f.write_str("translation"),
            Self::Rotation => f.write_str("rotation"),
            Self::Scale => f.write_str("scale"),
            Self::Parent => f.write_str("parent"),
            Self::Existence => f.write_str("existence"),
            Self::Component(name) => write!(f, "component {name}"),
        }
    }
}

impl WorldCommand {
    /// The entity the command acts on, if it acts on an existing one.
    ///
    /// `None` for [`Self::Spawn`], whose subject does not exist yet.
    pub fn entity(&self) -> Option<EntityId> {
        match self {
            Self::SetTranslation { entity, .. }
            | Self::Translate { entity, .. }
            | Self::SetRotation { entity, .. }
            | Self::SetScale { entity, .. }
            | Self::SetParent { entity, .. }
            | Self::Despawn { entity }
            | Self::SetComponent { entity, .. }
            | Self::AddComponent { entity, .. }
            | Self::RemoveComponent { entity, .. } => Some(*entity),
            Self::Spawn { .. } => None,
        }
    }

    /// What this command overwrites, if it overwrites anything.
    ///
    /// `None` means no write can be lost to it: [`Self::Translate`] accumulates
    /// and [`Self::Spawn`] creates. Both are order-dependent but neither
    /// discards another script's work.
    pub fn write_target(&self) -> Option<(EntityId, WriteTarget)> {
        let target = match self {
            Self::SetTranslation { .. } => WriteTarget::Translation,
            Self::SetRotation { .. } => WriteTarget::Rotation,
            Self::SetScale { .. } => WriteTarget::Scale,
            Self::SetParent { .. } => WriteTarget::Parent,
            Self::Despawn { .. } => WriteTarget::Existence,
            Self::SetComponent { component, .. }
            | Self::AddComponent { component, .. }
            | Self::RemoveComponent { component, .. } => WriteTarget::Component(component.clone()),
            Self::Translate { .. } | Self::Spawn { .. } => return None,
        };
        self.entity().map(|entity| (entity, target))
    }
}

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

//! The members the engine calls without being asked.
//!
//! An `on Damaged` fires because something raised `Damaged`. `Update` fires
//! because the frame advanced — nobody raises it, and no script can. That is the
//! whole difference, and it is why these are listed here rather than left to
//! look like ordinary methods that happen to have well-known names.
//!
//! # Why the signature is checked
//!
//! A method the engine calls is a method the author cannot see called. Writing
//! `void Update()` instead of `void Update(float dt)` is not a compile error in
//! any language that dispatches by name alone — it is a method that silently
//! never runs, which is the worst outcome available. So the shape is fixed here,
//! the checker holds authors to it, and the dispatcher trusts it.
//!
//! One table, read by both. Two lists of well-known names would drift, and the
//! drift would show up as a hook that stopped firing.

use crate::types::Ty;

/// A member the engine invokes on its own.
pub struct Lifecycle {
    /// The name an author writes.
    pub name: &'static str,
    /// The parameters it must take, in order.
    pub params: &'static [Ty],
    /// Whether the engine actually calls it yet.
    ///
    /// `false` says the hook is reserved but inert, and the checker warns rather
    /// than staying silent: a body that never runs is not something an author
    /// should have to discover by watching nothing happen.
    pub called: bool,
}

/// `void Update(float dt)` — once per frame, budget permitting.
pub static UPDATE: Lifecycle = Lifecycle {
    name: "Update",
    params: &[Ty::Float],
    called: true,
};

/// `void OnSpawn()` — once, when the behavior first runs on an entity.
///
/// Not the same moment as the field initialiser, which produces declared
/// defaults and runs again after a hot-reload. This runs once per *entity*, and
/// an edit to the script does not spawn it a second time.
pub static ON_SPAWN: Lifecycle = Lifecycle {
    name: "OnSpawn",
    params: &[],
    called: true,
};

/// `void OnDespawn()` — once, when the entity is on its way out.
///
/// Runs when the despawn is **decided**, not when it is performed: a script that
/// calls `Despawn(this)` gets this before the frame boundary applies it, while
/// the entity is still there to be read. An entity removed by something other
/// than a script is noticed the frame after it left the view, which is as early
/// as a lane that only reads a projection can know.
pub static ON_DESPAWN: Lifecycle = Lifecycle {
    name: "OnDespawn",
    params: &[],
    called: true,
};

/// `void FixedUpdate(float dt)` — reserved, not yet called.
///
/// It needs a fixed timestep to be called *at*, and the scripting agent
/// negotiates for frame time rather than declaring one. Listed anyway so an
/// author who writes it is told it will not run, instead of finding out from a
/// body that never executes.
pub static FIXED_UPDATE: Lifecycle = Lifecycle {
    name: "FixedUpdate",
    params: &[Ty::Float],
    called: false,
};

/// Every engine-invoked member.
pub static ALL: &[&Lifecycle] = &[&UPDATE, &ON_SPAWN, &ON_DESPAWN, &FIXED_UPDATE];

/// The lifecycle member a name denotes, if it denotes one.
pub fn of(name: &str) -> Option<&'static Lifecycle> {
    ALL.iter().copied().find(|hook| hook.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_known_name_resolves_and_an_ordinary_one_does_not() {
        assert!(of("Update").is_some());
        assert!(of("OnSpawn").is_some());
        assert!(of("Patrol").is_none());
    }

    /// The names are matched exactly. A method called `update` is the author's
    /// own, and quietly treating it as the hook would be a rule nobody wrote.
    #[test]
    fn the_match_is_case_sensitive() {
        assert!(of("update").is_none());
    }

    /// `Update` is the one hook whose parameter carries something, and getting
    /// it wrong is the mistake the whole table exists to catch.
    #[test]
    fn update_takes_the_frame_time() {
        assert_eq!(UPDATE.params, &[Ty::Float]);
        assert!(ON_SPAWN.params.is_empty());
    }
}

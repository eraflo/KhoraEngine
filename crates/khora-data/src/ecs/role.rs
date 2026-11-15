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

//! Whose work a `DataSystem` or a `Flow` is; every registration says it, with no default.
//!
//! A family's switch is the [`Activation`](khora_core::agent::Activation) resource.

use std::collections::BTreeSet;

use khora_core::agent::gorna::AgentId;

/// Whose work a registration is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// A data invariant: it runs every frame, whichever families are on.
    Upkeep,
    /// The work of the family named, or what that work reads: skipped while the family is off.
    ///
    /// Its first run after a skip is told it resumes.
    Serves(AgentId),
}

/// A registration serving a family no agent negotiates as: it would never be switched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServesUnknownFamily {
    /// The registration, by name.
    pub name: &'static str,
    /// The family it names.
    pub family: AgentId,
}

impl std::fmt::Display for ServesUnknownFamily {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "`{}` serves the family `{}`, which no registered agent is",
            self.name, self.family
        )
    }
}

/// Checks that every one of `roles` serves a family in `families`.
///
/// # Errors
/// The first, in the order given, that serves another family.
pub fn check_roles(
    roles: impl IntoIterator<Item = (&'static str, Role)>,
    families: &BTreeSet<AgentId>,
) -> Result<(), ServesUnknownFamily> {
    roles
        .into_iter()
        .find_map(|(name, role)| match role {
            Role::Serves(family) if !families.contains(&family) => {
                Some(ServesUnknownFamily { name, family })
            }
            _ => None,
        })
        .map_or(Ok(()), Err)
}

/// Returns every linked `DataSystem` and `Flow` registration, by name, with its role.
pub fn registered_roles() -> Vec<(&'static str, Role)> {
    let systems = inventory::iter::<crate::ecs::DataSystemRegistration>
        .into_iter()
        .map(|registration| (registration.name, registration.role));
    let flows = inventory::iter::<crate::flow::FlowRegistration>
        .into_iter()
        .map(|registration| (registration.name, registration.role));
    systems.chain(flows).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCRIPT: AgentId = AgentId::new("script");
    const PHYSICS: AgentId = AgentId::new("physics");

    /// **A role naming a registered family passes; so does upkeep.**
    #[test]
    fn roles_naming_registered_families_pass() {
        let families = BTreeSet::from([SCRIPT, PHYSICS]);

        let checked = check_roles(
            [
                ("transform_propagation", Role::Upkeep),
                ("script_intake", Role::Serves(SCRIPT)),
                ("physics_world_writeback", Role::Serves(PHYSICS)),
            ],
            &families,
        );

        assert_eq!(checked, Ok(()));
    }

    /// **The boot refuses a registration serving a family no agent is**, and
    /// names it.
    #[test]
    fn a_role_serving_an_unknown_family_is_refused() {
        let families = BTreeSet::from([SCRIPT]);
        let stranger = AgentId::new("stranger");

        let checked = check_roles(
            [
                ("script_intake", Role::Serves(SCRIPT)),
                ("lost_system", Role::Serves(stranger)),
            ],
            &families,
        );

        assert_eq!(
            checked,
            Err(ServesUnknownFamily {
                name: "lost_system",
                family: stranger,
            })
        );
    }

    /// **Every linked registration says its role**: the data layer's own
    /// systems and Flows are all there, by name.
    #[test]
    fn the_linked_registrations_are_listed_with_their_roles() {
        let roles = registered_roles();
        let role_of = |name: &str| {
            roles
                .iter()
                .find(|(registered, _)| *registered == name)
                .map(|(_, role)| *role)
        };

        assert_eq!(role_of("transform_propagation"), Some(Role::Upkeep));
        assert_eq!(role_of("physics_release"), Some(Role::Upkeep));
        assert_eq!(
            role_of("physics_debug_extraction"),
            Some(Role::Serves(khora_core::physics::PHYSICS))
        );
        assert_eq!(
            role_of("physics_world_writeback"),
            Some(Role::Serves(khora_core::physics::PHYSICS))
        );
        assert_eq!(
            role_of("script_systems"),
            Some(Role::Serves(khora_core::script::SCRIPT))
        );
        assert_eq!(
            role_of("script"),
            Some(Role::Serves(khora_core::script::SCRIPT)),
            "the script flow"
        );
        assert_eq!(
            role_of("audio_playback_writeback"),
            Some(Role::Serves(khora_core::audio::AUDIO))
        );
    }
}

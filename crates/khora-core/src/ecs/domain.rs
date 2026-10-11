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

//! The semantic domains a component belongs to.

use serde::{Deserialize, Serialize};

/// Defines the semantic domains a component can belong to.
///
/// A component registry maps every component to its domain, and the CRPECS
/// stores each component in a page of its domain. This grouping is the core
/// principle that gives the CRPECS fast, domain-specific queries. It lives in
/// the core so that whatever names a component's place — a registry, a script
/// declaring `[Domain(Physics)]`, a file recording it — names the same thing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SemanticDomain {
    /// For components related to position, physics, and the scene graph.
    Spatial,
    /// For components related to rendering, such as mesh and material handles.
    Render,
    /// For components related to audio, such as audio sources and listeners.
    Audio,
    /// For components related to physics simulation.
    Physics,
    /// For components driving the in-world UI subsystem (UiNode, UiText, etc).
    Ui,
    /// For gameplay logic attached to entities — which behavior an entity runs
    /// and the state that behavior keeps.
    ///
    /// Its own domain rather than a corner of `Spatial` because the two change
    /// on different clocks: a transform moves every frame, while which script an
    /// entity runs changes when a designer edits the scene. Sharing an epoch
    /// would mean one invalidates the other's cached projection for no reason.
    Script,
}

impl SemanticDomain {
    /// Number of semantic domains — sizes fixed per-domain tables such as a
    /// world's change epochs.
    pub const COUNT: usize = 6;

    /// Every domain, in [`index`](Self::index) order.
    pub const ALL: [SemanticDomain; Self::COUNT] = [
        SemanticDomain::Spatial,
        SemanticDomain::Render,
        SemanticDomain::Audio,
        SemanticDomain::Physics,
        SemanticDomain::Ui,
        SemanticDomain::Script,
    ];

    /// Dense index of this domain in `0..COUNT`, used to address fixed
    /// per-domain arrays without a `HashMap` lookup.
    pub const fn index(self) -> usize {
        match self {
            SemanticDomain::Spatial => 0,
            SemanticDomain::Render => 1,
            SemanticDomain::Audio => 2,
            SemanticDomain::Physics => 3,
            SemanticDomain::Ui => 4,
            SemanticDomain::Script => 5,
        }
    }

    /// The domain's name as Ergon spells it in `[Domain(…)]`: `"Physics"`.
    pub const fn name(self) -> &'static str {
        match self {
            SemanticDomain::Spatial => "Spatial",
            SemanticDomain::Render => "Render",
            SemanticDomain::Audio => "Audio",
            SemanticDomain::Physics => "Physics",
            SemanticDomain::Ui => "Ui",
            SemanticDomain::Script => "Script",
        }
    }

    /// The domain spelled `name`, as [`name`](Self::name) spells it.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|domain| domain.name() == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_domain_is_listed_at_its_index() {
        for (index, domain) in SemanticDomain::ALL.into_iter().enumerate() {
            assert_eq!(domain.index(), index);
        }
    }

    #[test]
    fn a_name_reads_back_as_its_domain() {
        for domain in SemanticDomain::ALL {
            assert_eq!(SemanticDomain::from_name(domain.name()), Some(domain));
        }
    }
}

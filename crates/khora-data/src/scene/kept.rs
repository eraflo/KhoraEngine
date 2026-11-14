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

//! Which components a document keeps: a scene's, or a game save's.

use super::component_registration::ComponentRegistration;

/// Which components a document keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kept {
    /// A scene: what an author or a tool wrote.
    Scene,
    /// A game save: that, and the state the engine wrote while running that
    /// play needs to resume.
    Game,
}

impl Kept {
    /// Whether this kind of document keeps `reg`'s component.
    pub(crate) fn keeps(self, reg: &ComponentRegistration) -> bool {
        match self {
            Self::Scene => reg.is_saved(),
            Self::Game => reg.is_saved() || reg.is_resumed(),
        }
    }
}

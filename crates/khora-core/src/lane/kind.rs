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

//! The kinds of lane — which agent negotiates for them.

/// Classification of lane types, used for routing and filtering.
///
/// Agents use this to identify compatible lanes during GORNA negotiation
/// and lane selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LaneKind {
    /// Main scene rendering (forward, deferred, etc.)
    Render,
    /// Shadow map generation
    Shadow,
    /// Physics simulation
    Physics,
    /// Audio mixing and spatialization
    Audio,
    /// Asset loading and processing
    Asset,
    /// Scene serialization/deserialization
    Scene,
    /// ECS maintenance (compaction, garbage collection)
    Ecs,
    /// User interface layout and interaction
    Ui,
    /// Gameplay scripts
    Script,
}

impl std::fmt::Display for LaneKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LaneKind::Render => write!(f, "Render"),
            LaneKind::Shadow => write!(f, "Shadow"),
            LaneKind::Physics => write!(f, "Physics"),
            LaneKind::Audio => write!(f, "Audio"),
            LaneKind::Asset => write!(f, "Asset"),
            LaneKind::Scene => write!(f, "Scene"),
            LaneKind::Ecs => write!(f, "ECS"),
            LaneKind::Ui => write!(f, "UI"),
            LaneKind::Script => write!(f, "Script"),
        }
    }
}

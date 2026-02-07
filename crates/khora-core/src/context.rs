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

//! Core engine context providing access to foundational subsystems.

use crate::renderer::GraphicsDevice;
use std::any::Any;
use std::sync::Arc;

/// Engine context providing access to various subsystems.
///
/// This structure is shared across both the Strategic Brain (Agents)
/// and the user-facing application logic.
pub struct EngineContext<'a> {
    /// The graphics device used for rendering.
    pub graphics_device: Arc<dyn GraphicsDevice>,

    /// A type-erased pointer to the main ECS World.
    /// This allows agents to access data without khora-core depending on khora-data.
    pub world: Option<&'a mut dyn Any>,

    /// A type-erased pointer to the asset registry.
    pub assets: Option<&'a dyn Any>,
}

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

use crate::service_registry::ServiceRegistry;
use std::any::Any;

/// Engine context providing access to various subsystems.
///
/// This structure is shared across both the Strategic Brain (Agents)
/// and the user-facing application logic.
///
/// # Design
///
/// Subsystem-specific services (e.g., `GraphicsDevice`, `RenderSystem`) are
/// accessed through the generic [`ServiceRegistry`] instead of named fields.
/// This respects the Interface Segregation Principle: each agent fetches only
/// the services it needs, and adding new services never changes this struct.
pub struct EngineContext<'a> {
    /// A type-erased pointer to the main ECS World.
    /// This allows agents to access data without khora-core depending on khora-data.
    pub world: Option<&'a mut dyn Any>,

    /// Generic service registry — agents fetch typed services as needed.
    pub services: ServiceRegistry,
}

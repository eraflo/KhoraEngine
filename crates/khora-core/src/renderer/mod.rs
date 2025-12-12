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

//! Provides the public, backend-agnostic rendering contracts for the Khora Engine.
//!
//! This module defines the "common language" for all rendering operations. It contains
//! the abstract `traits` (like [`GraphicsDevice`]), data structures (like [`BufferDescriptor`]),
//! and error types that form the stable, public-facing API for rendering.
//!
//! Following the CLAD architecture, this module defines the 'what' of rendering,
//! while the 'how' is handled by a concrete backend implementation in the `khora-infra`
//! crate (e.g., a WGPU backend) which implements these traits. The `khora-lanes`
//! and `khora-agents` then use these traits to perform their work without needing to
//! know the specifics of the underlying graphics API.

pub mod api;
pub mod error;
pub mod light;
pub mod traits;

// Re-export the most important traits and types for easier use.
pub use self::api::*;
pub use self::error::{PipelineError, RenderError, ResourceError, ShaderError};
pub use self::light::{DirectionalLight, LightType, PointLight, SpotLight};
pub use self::traits::{GraphicsDevice, RenderSystem};

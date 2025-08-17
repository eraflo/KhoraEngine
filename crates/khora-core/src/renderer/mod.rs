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

//! The public, backend-agnostic rendering API for Khora Engine.
//!
//! This module defines the 'what' of rendering (traits, data structures, error types)
//! without specifying the 'how' (which is handled by a concrete implementation in `khora-infra`).

pub mod api;
pub mod error;
pub mod traits;

// Re-export the most important traits and types for easier use.
pub use self::api::*;
pub use self::error::{PipelineError, RenderError, ResourceError, ShaderError};
pub use self::traits::{GraphicsDevice, RenderSystem};

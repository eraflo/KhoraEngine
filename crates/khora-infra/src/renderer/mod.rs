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

//! Renderer infrastructure module.
//!
//! Provides concrete implementations of rendering systems including
//! text rendering, texture atlasing, and GPU resource management.

pub mod custom;
/// Text rendering implementation.
pub mod text;
/// Utility functions and types for rendering.
pub mod util;

pub use text::StandardTextRenderer;

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

//! Backend-agnostic rendering API.
//!
//! Organized into several logical sub-modules:
//!
//! - **[`device`]**: The graphics device: adapter, backend choice, settings, statistics.
//! - **[`frame`]**: One frame of rendering: the render context and the per-frame state.
//! - **[`resource`]**: GPU handles (Buffer, Texture) and their descriptors and formats.
//! - **[`shader`]**: Shader modules and sources, stages, global defs, variant keys.
//! - **[`command`]**: Command recording, encoders, and pass definitions.
//! - **[`pipeline`]**: Static pipeline state, layouts, and configuration.
//! - **[`gpu_scene`]**: What a frame draws: meshes, render objects, per-model uniforms.
//! - **[`material`]**: The material bind group and the GPU material data.
//! - **[`util`]**: Generic utility types and containers.

pub mod command;
pub mod device;
pub mod frame;
pub mod gpu_scene;
pub mod ibl;
pub mod material;
pub mod pipeline;
pub mod resource;
pub mod shader;
pub mod shadow;
pub mod text;
pub mod util;

pub use shader::ShaderDefs;

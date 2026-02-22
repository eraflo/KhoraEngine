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
//! - **[`core`]**: Infrastructure, backend abstraction, and global state.
//! - **[`resource`]**: GPU handles (Buffer, Texture) and their descriptors.
//! - **[`command`]**: Command recording, encoders, and pass definitions.
//! - **[`pipeline`]**: Static pipeline state, layouts, and configuration.
//! - **[`scene`]**: High-level rendering entities (Light, Mesh, RenderObject).
//! - **[`util`]**: Generic utility types and containers.

pub mod command;
pub mod core;
pub mod pipeline;
pub mod resource;
pub mod scene;
pub mod util;

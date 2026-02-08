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

//! Contains all the public data structures and enums for the backend-agnostic rendering API.
//!
//! This module defines the "nouns" of the rendering language: the descriptors used
//! to create resources (e.g., [`BufferDescriptor`], [`TextureDescriptor`]), the handles
//! used to identify them (e.g., [`BufferId`], [`TextureId`]), and the various enums
//! that configure their behavior (e.g., [`TextureFormat`]).
//!
//! These types are used in the method signatures of the traits defined in the
//! parent module's `traits` submodule.

pub mod backend;
pub mod bind_group;
pub mod buffer;
pub mod command;
pub mod common;
pub mod compute;
pub mod context;
pub mod mesh;
pub mod pipeline;
pub mod shader;
pub mod texture;
pub mod uniform_ring_buffer;

pub use self::backend::*;
pub use self::bind_group::*;
pub use self::buffer::*;
pub use self::command::*;
pub use self::common::*;
pub use self::compute::*;
pub use self::context::*;
pub use self::mesh::*;
pub use self::pipeline::*;
pub use self::shader::*;
pub use self::texture::*;
pub use self::uniform_ring_buffer::*;

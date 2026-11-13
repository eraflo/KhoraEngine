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

//! GPU resource management.
//!
//! - [`ProjectionRegistry`]: what the GPU holds for the world's meshes and
//!   materials, in the engine-wide [`AssetStore`](crate::assets::AssetStore);
//!   the residencies (`crate::residency`) fill it.
//! - [`AssetEviction`]: frees what the GPU holds for assets no longer used.
//! - [`ibl`]: image-based lighting bakes.

pub mod eviction;
pub mod ibl;
pub mod projection;
#[cfg(test)]
pub(crate) mod tests;

pub use eviction::AssetEviction;
pub use ibl::{EnvironmentMap, IblBaker, IblSource};
pub use projection::ProjectionRegistry;

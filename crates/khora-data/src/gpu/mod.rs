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

//! What the GPU holds for the asset store's meshes and materials
//! ([`ProjectionRegistry`]), its release once no reference names them
//! ([`AssetEviction`]), and the image-based lighting bakes ([`ibl`]).

pub mod eviction;
pub mod ibl;
mod made_copies;
mod material_waits;
pub mod projection;

pub use eviction::AssetEviction;
pub use ibl::{EnvironmentMap, IblBaker, IblSource};
pub(crate) use made_copies::MadeCopies;
pub(crate) use material_waits::MaterialWaits;
pub use projection::ProjectionRegistry;

#[cfg(test)]
pub(crate) mod tests;

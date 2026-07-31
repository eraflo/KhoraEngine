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

//! Shared image-based lighting (IBL) API — the abstract contract the IBL
//! bake produces and every lit consumer reads.
//!
//! The IBL bake (procedural or image-based environment → diffuse irradiance
//! cube → prefiltered specular cube → split-sum BRDF LUT) runs once at
//! startup and publishes an [`IblGpuBindings`] the lit lanes bind at group 3.
//! Unlike shadow, whose bindings sit at fixed indices, the IBL block starts
//! at a lane-specific base index (see [`bindings`]).
//!
//! The split-sum evaluation math is shared across lanes in the WGSL lib
//! `lib/lighting/ibl.wgsl`; only the texture declarations differ per lane.

pub mod bindings;

pub use bindings::{
    fill_ibl_bind_group_entries, ibl_bind_group_layout_entries, IblGpuBindings, IBL_BINDING_COUNT,
};

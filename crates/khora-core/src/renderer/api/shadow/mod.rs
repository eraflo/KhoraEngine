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

//! Shared shadow API — the abstract contract every shadow strategy
//! produces and every lit consumer reads.
//!
//! All shadow strategies (`StandardShadowsLane`, `LowResShadowsLane`,
//! future variants) emit a [`ShadowFrame`] into the per-tick
//! [`OutputDeck`](crate::lane::OutputDeck) at the end of `execute()`.
//! Lit lanes read that slot and treat its contents opaquely — they never
//! inspect 2D-vs-cube atlas details.
//!
//! The bind-group contract for the "lit + shadow" group (group 3 of
//! every lit pipeline) lives in [`bindings`]:
//!
//! | Binding | Resource                             | Owner                |
//! |---------|--------------------------------------|----------------------|
//! | 0       | `LightingUniforms` uniform buffer    | lit lane             |
//! | 1       | 2D depth atlas (directional / spot)  | shadow strategy      |
//! | 2       | Comparison sampler                   | shadow strategy      |
//! | 3       | Cube depth atlas (point lights)      | shadow strategy      |
//!
//! Lit lanes call [`bindings::shadow_bind_group_layout_entries`] when
//! creating their pipeline layout, and
//! [`bindings::fill_shadow_bind_group_entries`] when building the
//! per-frame bind group. The matching WGSL — bindings + sampling
//! functions — lives under `khora-lanes/src/render_lane/shaders/lib/
//! shadow/` (`bindings.wgsl`, `sample_2d.wgsl`, `sample_cube.wgsl`)
//! and is composed into every lit shader at module-creation time by
//! the `ShaderRegistry`.

pub mod bindings;
mod entries;
mod frame;

pub use bindings::{
    fill_shadow_bind_group_entries, shadow_bind_group_layout_entries, ShadowGpuBindings,
};
pub use entries::{ShadowEntries, ShadowEntry};
pub use frame::ShadowFrame;

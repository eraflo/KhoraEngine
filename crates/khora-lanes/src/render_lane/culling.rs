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

//! Which faces of a mesh a lit draw culls.

use khora_core::renderer::api::pipeline::enums::CullMode;

/// The faces a lit draw of a mesh culls.
///
/// A double-sided material culls nothing. A single-sided one culls its back
/// faces, except when the draw is mirrored: a negative determinant turns its
/// counter-clockwise faces clockwise, so the faces seen from outside are the
/// "front" ones the pipeline would cull. Culling front faces instead keeps
/// the front face counter-clockwise for every pipeline, so the cull mode alone
/// tells the pipelines apart in their cache key. A shader reading
/// `front_facing` would see `false` on the visible faces of a mirrored draw.
pub(crate) fn mesh_cull_mode(double_sided: bool, mirrored: bool) -> Option<CullMode> {
    match (double_sided, mirrored) {
        (true, _) => None,
        (false, false) => Some(CullMode::Back),
        (false, true) => Some(CullMode::Front),
    }
}

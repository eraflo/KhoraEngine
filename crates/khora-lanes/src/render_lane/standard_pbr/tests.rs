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

use super::*;
use khora_core::lane::Lane;

#[test]
fn standard_pbr_lane_strategy_name() {
    let lane = StandardPbrLane::default();
    assert_eq!(lane.strategy_name(), "StandardPbr");
    assert_eq!(lane.lane_kind(), khora_core::lane::LaneKind::Render);
}

#[test]
fn standard_pbr_pipeline_binds_the_mesh_vertex_for_every_material_state() {
    use khora_core::renderer::api::shader::ShaderVariantKey;
    let device = crate::tests::SpecOnlyDevice;
    for double_sided in [false, true] {
        for blend in [false, true] {
            let spec =
                super::gpu::pipeline_spec(&device, ShaderVariantKey::empty(), double_sided, blend);
            crate::tests::assert_binds_the_mesh_vertex(&spec);
        }
    }
}

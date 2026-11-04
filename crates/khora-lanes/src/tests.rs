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

//! Test-only helpers shared by the lanes' unit tests.

use khora_core::renderer::api::gpu_scene::Mesh;
use khora_core::renderer::api::pipeline::PipelineSpec;

/// Asserts that a mesh pipeline binds exactly one vertex buffer laid out as
/// the engine mesh vertex ([`Mesh::VERTEX_ATTRIBUTES`], [`Mesh::VERTEX_STRIDE`]),
/// i.e. the layout [`Mesh::vertex_buffer_layout`] returns.
pub(crate) fn assert_binds_the_mesh_vertex(spec: &PipelineSpec) {
    assert_eq!(
        spec.vertex_buffers.len(),
        1,
        "{}: a mesh pipeline binds one vertex buffer",
        spec.label
    );
    let declared = &spec.vertex_buffers[0];
    assert_eq!(
        declared.array_stride,
        Mesh::VERTEX_STRIDE,
        "{}: vertex stride differs from the mesh vertex",
        spec.label
    );
    assert_eq!(
        declared.attributes.as_ref(),
        Mesh::VERTEX_ATTRIBUTES,
        "{}: vertex attributes differ from the mesh vertex",
        spec.label
    );

    let contract = Mesh::vertex_buffer_layout();
    assert_eq!(
        declared.array_stride, contract.array_stride,
        "{}",
        spec.label
    );
    assert_eq!(declared.step_mode, contract.step_mode, "{}", spec.label);
    assert_eq!(
        declared.attributes.as_ref(),
        contract.attributes.as_ref(),
        "{}",
        spec.label
    );
}

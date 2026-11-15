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

//! The wgpu descriptor an engine buffer descriptor becomes.

use super::super::*;

fn a_descriptor(usage: api_buf::BufferUsage) -> api_buf::BufferDescriptor<'static> {
    api_buf::BufferDescriptor {
        label: Some(std::borrow::Cow::Borrowed("test")),
        size: 256,
        usage,
        mapped_at_creation: false,
    }
}

/// A vertex buffer reaches wgpu as a vertex buffer, never an index one.
///
/// Khora numbers `VERTEX` at bit 4, wgpu at bit 5: a raw bit copy swaps them.
#[test]
fn a_vertex_buffer_stays_a_vertex_buffer() {
    let source = a_descriptor(api_buf::BufferUsage::VERTEX);
    let built = buffer_descriptor(&source);

    assert_eq!(built.usage, wgpu::BufferUsages::VERTEX);
    assert!(
        !built.usage.contains(wgpu::BufferUsages::INDEX),
        "the exact swap that was reported"
    );
}

#[test]
fn an_index_buffer_stays_an_index_buffer() {
    let source = a_descriptor(api_buf::BufferUsage::INDEX);
    let built = buffer_descriptor(&source);

    assert_eq!(built.usage, wgpu::BufferUsages::INDEX);
    assert!(!built.usage.contains(wgpu::BufferUsages::VERTEX));
}

/// A vertex buffer that is also a copy target keeps `VERTEX` with no `INDEX` bit.
///
/// The engine's text renderer asks for this combination.
#[test]
fn a_vertex_buffer_that_is_also_a_copy_target() {
    let source = a_descriptor(api_buf::BufferUsage::VERTEX | api_buf::BufferUsage::COPY_DST);
    let built = buffer_descriptor(&source);

    assert_eq!(
        built.usage,
        wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        "no INDEX bit needed to keep VERTEX"
    );
}

/// The label, the size and the mapping flag are carried across unchanged.
#[test]
fn the_rest_of_the_descriptor_is_carried_across() {
    let mut source = a_descriptor(api_buf::BufferUsage::UNIFORM);
    source.mapped_at_creation = true;

    let built = buffer_descriptor(&source);

    assert_eq!(built.label, Some("test"));
    assert_eq!(built.size, 256);
    assert!(built.mapped_at_creation);
}

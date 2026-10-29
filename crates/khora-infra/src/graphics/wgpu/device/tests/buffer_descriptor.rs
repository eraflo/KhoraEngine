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

/// **Issue #279.** Reported from the outside against a Metal build: a
/// vertex buffer reached the driver marked as an index buffer.
///
/// The cause was `wgpu::BufferUsages::from_bits_truncate(usage.bits())`
/// here, where Khora numbers `VERTEX` at bit 4 and wgpu numbers it at
/// bit 5. The conversion that does it properly already existed and was
/// used by `create_buffer_init` fifty lines below; this path simply did
/// not call it.
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

/// **The reporter's case, with their workaround removed.**
///
/// `lana-khora-spike` asked for `VERTEX | INDEX | COPY_DST` on a vertex
/// buffer so that the raw bit copy would yield wgpu `INDEX | VERTEX`, and
/// the `VERTEX` flag would survive whichever way the bits landed. Their
/// comment says to drop the `INDEX` bit once `create_buffer` calls
/// `into_wgpu`. This is that call, without the bit.
///
/// The engine asks for the same combination in its own text renderer
/// (`khora-infra/src/graphics/text/standard.rs`), which is why the defect was not
/// specific to their project — it simply never surfaced there, because
/// nothing queues text in the sandbox and the buffer is bound only when
/// something does.
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

/// The rest of the descriptor is carried across unchanged — a fix that
/// quietly dropped the label or the mapping flag would trade one silent
/// failure for another.
#[test]
fn the_rest_of_the_descriptor_is_carried_across() {
    let mut source = a_descriptor(api_buf::BufferUsage::UNIFORM);
    source.mapped_at_creation = true;

    let built = buffer_descriptor(&source);

    assert_eq!(built.label, Some("test"));
    assert_eq!(built.size, 256);
    assert!(built.mapped_at_creation);
}

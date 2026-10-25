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

//! The vertex and index buffers of `StandardTextRenderer` hold exactly the
//! frame's glyphs: they grow when a frame needs more room, are reused when it
//! needs less, and the indices always name the frame's own vertices.

use super::*;

/// The vertex and index buffers the renderer currently owns: the ones of its
/// usage that were created and not destroyed.
fn live_glyph_buffers(device: &RecordingDevice) -> (Vec<BufferId>, Vec<BufferId>) {
    let destroyed = device.destroyed();
    let live = |usage: BufferUsage| -> Vec<BufferId> {
        device
            .buffers()
            .into_iter()
            .filter(|b| b.usage.contains(usage) && !destroyed.contains(&b.id))
            .map(|b| b.id)
            .collect()
    };
    (live(BufferUsage::VERTEX), live(BufferUsage::INDEX))
}

/// The vertex and index buffers the last recorded pass binds.
fn bound_buffers(passes: &[PassRecord]) -> (BufferId, BufferId) {
    let pass = passes.last().expect("the flush draws");
    let vertices = pass.vertex_buffer.expect("the pass binds a vertex buffer");
    let (indices, _) = pass.index_buffer.expect("the pass binds an index buffer");
    (vertices, indices)
}

/// The `u32` indices last uploaded to `buffer`.
fn uploaded_indices(device: &RecordingDevice, buffer: BufferId) -> Vec<u32> {
    let bytes = device
        .writes_to(buffer)
        .last()
        .cloned()
        .expect("the indices are uploaded");
    bytes
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect()
}

/// The number of vertices last uploaded to `buffer`.
fn uploaded_vertex_count(device: &RecordingDevice, buffer: BufferId) -> usize {
    let bytes = device
        .writes_to(buffer)
        .last()
        .map(Vec::len)
        .expect("the vertices are uploaded");
    bytes / std::mem::size_of::<crate::graphics::text::standard::TextVertex>()
}

#[test]
fn a_frame_that_fills_the_initial_buffers_exactly_draws_every_glyph_without_growing() {
    let renderer = renderer();
    let device = RecordingDevice::default();
    // The initial buffers hold 512 glyphs: 64 KiB of vertices, 12 KiB of indices.
    queue(&renderer, &glyphs(512), Vec2::new(0.0, 0.0));

    let passes = flush(&renderer, &device, extent(1920, 1080));

    assert_eq!(indices_drawn(&passes), 512 * 6, "every glyph is drawn");
    assert!(
        device.destroyed().is_empty(),
        "a frame that fits the buffers replaces none"
    );
    let (vertices, indices) = bound_buffers(&passes);
    assert_eq!(device.buffer(vertices).size, 512 * 4 * 32);
    assert_eq!(device.buffer(indices).size, 512 * 6 * 4);
}

#[test]
fn a_frame_one_glyph_past_the_initial_room_grows_both_buffers() {
    let renderer = renderer();
    let device = RecordingDevice::default();
    queue(&renderer, &glyphs(513), Vec2::new(0.0, 0.0));

    let passes = flush(&renderer, &device, extent(1920, 1080));

    assert_eq!(indices_drawn(&passes), 513 * 6, "every glyph is drawn");
    let (vertices, indices) = bound_buffers(&passes);
    assert!(device.buffer(vertices).size >= 513 * 4 * 32);
    assert!(device.buffer(indices).size >= 513 * 6 * 4);
    let (live_vertices, live_indices) = live_glyph_buffers(&device);
    assert_eq!(
        live_vertices,
        [vertices],
        "the outgrown vertex buffer is destroyed"
    );
    assert_eq!(
        live_indices,
        [indices],
        "the outgrown index buffer is destroyed"
    );
}

#[test]
fn a_smaller_frame_after_growth_reuses_the_grown_buffers() {
    let renderer = renderer();
    let device = RecordingDevice::default();
    queue(&renderer, &glyphs(2_000), Vec2::new(0.0, 0.0));
    let large = flush(&renderer, &device, extent(1920, 1080));
    let grown = bound_buffers(&large);
    let created = device.buffers().len();

    queue(&renderer, "Hi", Vec2::new(0.0, 0.0));
    let small = flush(&renderer, &device, extent(1920, 1080));

    assert_eq!(bound_buffers(&small), grown, "the grown buffers stay bound");
    assert_eq!(
        device.buffers().len(),
        created,
        "a smaller frame creates no buffer"
    );
    assert_eq!(
        indices_drawn(&small),
        12,
        "only the small frame's glyphs are drawn"
    );
}

#[test]
fn repeated_growth_destroys_each_outgrown_buffer_once_and_never_a_live_one() {
    let renderer = renderer();
    let device = RecordingDevice::default();

    for count in [600, 3_000, 10_000] {
        queue(&renderer, &glyphs(count), Vec2::new(0.0, 0.0));
        let passes = flush(&renderer, &device, extent(1920, 1080));
        assert_eq!(
            indices_drawn(&passes),
            (count * 6) as u32,
            "every glyph of a {count}-glyph frame is drawn"
        );

        let (vertices, indices) = bound_buffers(&passes);
        let destroyed = device.destroyed();
        assert!(
            !destroyed.contains(&vertices),
            "the bound vertex buffer is alive"
        );
        assert!(
            !destroyed.contains(&indices),
            "the bound index buffer is alive"
        );
        let mut unique = destroyed.clone();
        unique.sort_by_key(|b| b.0);
        unique.dedup();
        assert_eq!(
            unique.len(),
            destroyed.len(),
            "no buffer is destroyed twice"
        );
        let (live_vertices, live_indices) = live_glyph_buffers(&device);
        assert_eq!(live_vertices, [vertices], "one vertex buffer is alive");
        assert_eq!(live_indices, [indices], "one index buffer is alive");
    }
}

#[test]
fn glyphs_that_do_not_rasterise_leave_no_gap_in_the_vertices_their_indices_name() {
    let renderer = renderer();
    let device = RecordingDevice::default();
    // `é`, `€` and `日` have no built-in glyph; the space has a blank one.
    queue(&renderer, "AéB", Vec2::new(0.0, 0.0));
    queue(&renderer, "€ C", Vec2::new(0.0, 20.0));
    queue(&renderer, "日", Vec2::new(0.0, 40.0));

    let passes = flush(&renderer, &device, extent(800, 600));

    // A, B, space, C: four glyphs.
    assert_eq!(indices_drawn(&passes), 4 * 6);
    let (vertices, indices) = bound_buffers(&passes);
    let vertex_count = uploaded_vertex_count(&device, vertices);
    assert_eq!(
        vertex_count,
        4 * 4,
        "four vertices a drawn glyph, none for the others"
    );
    let expected: Vec<u32> = (0..4u32)
        .flat_map(|g| {
            let b = g * 4;
            [b, b + 1, b + 2, b, b + 2, b + 3]
        })
        .collect();
    assert_eq!(uploaded_indices(&device, indices), expected);
}

#[test]
fn a_text_with_no_rasterisable_glyph_begins_no_pass_and_leaves_the_next_frame_whole() {
    let renderer = renderer();
    let device = RecordingDevice::default();
    queue(&renderer, "éàü", Vec2::new(0.0, 0.0));

    let passes = flush(&renderer, &device, extent(800, 600));
    assert!(passes.is_empty(), "nothing to draw begins no pass");

    queue(&renderer, "Hi", Vec2::new(0.0, 0.0));
    let passes = flush(&renderer, &device, extent(800, 600));
    assert_eq!(
        indices_drawn(&passes),
        12,
        "the next frame draws its own text"
    );
}

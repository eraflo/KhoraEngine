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

//! A UI quad is four vertices drawn as a triangle strip, blended over what the
//! frame already holds.
//!
//! The vertex shader emits one corner per vertex index; as a `TriangleList`
//! four vertices make one triangle and a stray vertex, so every quad drew as
//! half a quad. Without blending, the anti-aliased edge and every translucent
//! colour overwrote the scene instead of covering it.

use super::super::{ui_pipeline_spec, UiRenderLane};
use crate::tests::{ledger_context, LedgerDevice, LedgerPipelines};
use khora_core::lane::{ColorTarget, ColorTargetSize, Lane, LaneContext};
use khora_core::math::{Extent2D, Vec2};
use khora_core::renderer::api::pipeline::{BlendFactor, BlendOperation, PrimitiveTopology};
use khora_core::renderer::api::resource::TextureViewId;
use khora_core::renderer::GraphicsDevice;
use khora_data::ui::{ExtractedUiNode, UiAtlasMap, UiScene};
use std::sync::Arc;

fn node_at(pos: Vec2) -> ExtractedUiNode {
    ExtractedUiNode {
        pos,
        size: Vec2::new(100.0, 20.0),
        color: None,
        border: None,
        image: None,
        z_index: 0,
    }
}

#[test]
fn ui_pipeline_draws_quads_as_triangle_strips() {
    let spec = ui_pipeline_spec(&crate::tests::SpecOnlyDevice);

    assert_eq!(
        spec.primitive.topology,
        PrimitiveTopology::TriangleStrip,
        "four corners per instance make a quad only as a triangle strip"
    );

    assert_eq!(
        spec.color_targets.len(),
        1,
        "the UI draws one colour target"
    );
    let blend = spec.color_targets[0]
        .blend
        .expect("the UI blends over the frame: anti-aliased edges and translucent colours");
    // The fragment shader returns straight (non-premultiplied) alpha, as the
    // text and the alpha-blended lit pipelines do.
    assert_eq!(
        (
            blend.color.src_factor,
            blend.color.dst_factor,
            blend.color.operation
        ),
        (
            BlendFactor::SrcAlpha,
            BlendFactor::OneMinusSrcAlpha,
            BlendOperation::Add
        ),
        "colour blends as straight alpha over the destination"
    );
    assert_eq!(
        blend.alpha.dst_factor,
        BlendFactor::OneMinusSrcAlpha,
        "the destination alpha is covered by the quad's alpha, not replaced"
    );

    // The draw that goes with the topology: four vertices, one instance per quad.
    let device = Arc::new(LedgerDevice::default());
    let pipelines = Arc::new(LedgerPipelines::default());
    let lane = UiRenderLane::new();
    lane.on_initialize(&mut ledger_context(&device, &pipelines))
        .expect("the UI lane initialises on a ledger device");

    let scene = UiScene {
        nodes: vec![node_at(Vec2::new(0.0, 0.0)), node_at(Vec2::new(0.0, 30.0))],
        texts: vec![],
    };
    let atlas_map = UiAtlasMap::default();
    let mut encoder = device.create_command_encoder(Some("ui test"));
    {
        let mut ctx = LaneContext::new();
        ctx.insert(Arc::clone(&device) as Arc<dyn GraphicsDevice>);
        ctx.insert_ref(&scene);
        ctx.insert_ref(&atlas_map);
        ctx.insert_slot(encoder.as_mut());
        ctx.insert(ColorTarget(TextureViewId(1)));
        ctx.insert(ColorTargetSize(Extent2D {
            width: 800,
            height: 600,
        }));
        lane.execute(&mut ctx).expect("a UI frame with two quads");
    }

    assert_eq!(
        device.draws(),
        [(0..4, 0..2)],
        "one instanced draw: four strip vertices per quad, one instance per node"
    );
}

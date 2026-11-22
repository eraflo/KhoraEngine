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

//! A render pass begun through the device's encoder, on a real adapter: every
//! attachment's view is checked first, and a pass naming a view the device
//! does not hold is refused whole, saying which attachment. Without any
//! adapter each test logs why and returns.

use std::borrow::Cow;
use std::sync::{Arc, Mutex};

use khora_core::math::{Extent3D, LinearRgba};
use khora_core::renderer::api::command::{
    LoadOp, Operations, RenderPassColorAttachment, RenderPassDepthStencilAttachment,
    RenderPassDescriptor, StoreOp,
};
use khora_core::renderer::api::resource::{
    ImageAspect, SampleCount, TextureDescriptor, TextureDimension, TextureFormat, TextureUsage,
    TextureViewDescriptor, TextureViewDimension, TextureViewId,
};
use khora_core::renderer::traits::RenderPassError;
use khora_core::renderer::GraphicsDevice;

use super::super::WgpuDevice;
use crate::graphics::wgpu::resilience::GpuHealth;

/// The side of every attachment the tests make.
const SIDE: u32 = 8;
/// An id no view of the device was ever given.
const NEVER_MADE: TextureViewId = TextureViewId(usize::MAX);

/// The device under test with the errors wgpu reported on it, holding the
/// process's turn on the GPU; `None` (logged) without any adapter.
struct Gpu {
    device: WgpuDevice,
    errors: Arc<Mutex<Vec<String>>>,
    _turn: std::sync::MutexGuard<'static, ()>,
}

impl Gpu {
    /// Makes a device of its own for `test`, or `None` (logged) without any adapter.
    fn new(test: &str) -> Option<Self> {
        let turn = crate::graphics::wgpu::tests::gpu::exclusive_gpu();
        let Some(adapter) = crate::graphics::wgpu::tests::gpu::test_adapter() else {
            log::warn!("{test}: no wgpu adapter, skipped");
            return None;
        };
        let (device, queue) =
            match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some(test),
                required_features: wgpu::Features::empty(),
                required_limits: adapter.limits(),
                experimental_features: wgpu::ExperimentalFeatures::default(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            })) {
                Ok(pair) => pair,
                Err(e) => {
                    log::warn!("{test}: no device ({e}), skipped");
                    return None;
                }
            };
        // A pass begun with a bad attachment would show up here, not as a
        // panic on whichever thread wgpu reports it.
        let errors = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&errors);
        device.on_uncaptured_error(Arc::new(move |e| {
            if let Ok(mut errors) = sink.lock() {
                errors.push(format!("{e:?}"));
            }
        }));
        Some(Self {
            device: WgpuDevice::new(device, queue, Arc::new(GpuHealth::default())),
            errors,
            _turn: turn,
        })
    }

    /// Makes a texture of `format` usable as an attachment and returns its view.
    fn attachment(&self, format: TextureFormat) -> TextureViewId {
        let texture = self
            .device
            .create_texture(&TextureDescriptor {
                label: Some(Cow::Borrowed("render pass test attachment")),
                size: Extent3D {
                    width: SIDE,
                    height: SIDE,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: SampleCount::X1,
                dimension: TextureDimension::D2,
                format,
                usage: TextureUsage::RENDER_ATTACHMENT,
                view_formats: Cow::Borrowed(&[]),
            })
            .expect("the device makes an attachment texture");
        self.device
            .create_texture_view(
                texture,
                &TextureViewDescriptor {
                    label: Some(Cow::Borrowed("render pass test view")),
                    format: None,
                    dimension: Some(TextureViewDimension::D2),
                    aspect: ImageAspect::All,
                    base_mip_level: 0,
                    mip_level_count: None,
                    base_array_layer: 0,
                    array_layer_count: None,
                },
            )
            .expect("the device makes the attachment's view")
    }

    /// Makes a colour attachment's view, then destroys it.
    fn destroyed_view(&self) -> TextureViewId {
        let view = self.attachment(TextureFormat::Rgba8Unorm);
        self.device
            .destroy_texture_view(view)
            .expect("the device destroys the view it made");
        view
    }

    /// Begins a pass over `colors` and `depth` on a fresh encoder and returns
    /// the refusal, or `None` when the pass was begun. The encoder is dropped
    /// unsubmitted.
    fn refusal(
        &self,
        colors: &[RenderPassColorAttachment<'_>],
        depth: Option<RenderPassDepthStencilAttachment<'_>>,
    ) -> Option<RenderPassError> {
        let mut encoder = self.device.create_command_encoder(Some("render pass test"));
        let result = encoder
            .begin_render_pass(&RenderPassDescriptor {
                label: Some("render pass test"),
                color_attachments: colors,
                depth_stencil_attachment: depth,
            })
            .map(drop);
        result.err()
    }

    /// The validation and internal errors wgpu reported, if any.
    fn reported_errors(&self) -> Vec<String> {
        self.errors
            .lock()
            .map(|errors| errors.clone())
            .unwrap_or_else(|_| vec!["the error list was poisoned".to_owned()])
    }
}

/// A colour attachment over `view`, cleared, resolving into `resolve`.
fn color<'a>(
    view: &'a TextureViewId,
    resolve: Option<&'a TextureViewId>,
) -> RenderPassColorAttachment<'a> {
    RenderPassColorAttachment {
        view,
        resolve_target: resolve,
        ops: Operations {
            load: LoadOp::Clear(LinearRgba::BLACK),
            store: StoreOp::Store,
        },
        base_array_layer: 0,
        base_mip_level: 0,
    }
}

/// A depth attachment over `view`, cleared to the far plane.
fn depth(view: &TextureViewId) -> RenderPassDepthStencilAttachment<'_> {
    RenderPassDepthStencilAttachment {
        view,
        depth_ops: Some(Operations {
            load: LoadOp::Clear(1.0),
            store: StoreOp::Store,
        }),
        stencil_ops: None,
        base_array_layer: 0,
    }
}

/// **A pass whose views all stand is begun.** The control for the refusals
/// below: colour and depth the device holds open a pass, and wgpu reports
/// nothing.
#[test]
fn a_pass_whose_views_all_stand_is_begun() {
    let Some(gpu) = Gpu::new("a_pass_whose_views_all_stand_is_begun") else {
        return;
    };
    let target = gpu.attachment(TextureFormat::Rgba8Unorm);
    let depth_view = gpu.attachment(TextureFormat::Depth32Float);

    let refusal = gpu.refusal(&[color(&target, None)], Some(depth(&depth_view)));

    assert_eq!(refusal, None);
    // wgpu reports a pass's validation errors when its encoder finishes.
    let errors = gpu.submitted(&[color(&target, None)], Some(depth(&depth_view)));
    assert_eq!(errors, Vec::<String>::new());
}

/// **A destroyed colour view refuses the pass, naming its attachment.**
#[test]
fn a_destroyed_colour_view_refuses_the_pass() {
    let Some(gpu) = Gpu::new("a_destroyed_colour_view_refuses_the_pass") else {
        return;
    };
    let gone = gpu.destroyed_view();

    let refusal = gpu.refusal(&[color(&gone, None)], None);

    assert_eq!(
        refusal,
        Some(RenderPassError::UnknownColorView {
            index: 0,
            view: gone
        })
    );
    assert_eq!(gpu.reported_errors(), Vec::<String>::new());
}

/// **A destroyed resolve target refuses the pass**: a pass that rendered
/// without resolving would leave its target as it was.
#[test]
fn a_destroyed_resolve_target_refuses_the_pass() {
    let Some(gpu) = Gpu::new("a_destroyed_resolve_target_refuses_the_pass") else {
        return;
    };
    let target = gpu.attachment(TextureFormat::Rgba8Unorm);
    let gone = gpu.destroyed_view();

    let refusal = gpu.refusal(&[color(&target, Some(&gone))], None);

    assert_eq!(
        refusal,
        Some(RenderPassError::UnknownResolveTarget {
            index: 0,
            view: gone
        })
    );
    assert_eq!(gpu.reported_errors(), Vec::<String>::new());
}

/// **A destroyed depth view refuses the pass**: a pass begun without depth
/// does not match a pipeline built with depth.
#[test]
fn a_destroyed_depth_view_refuses_the_pass() {
    let Some(gpu) = Gpu::new("a_destroyed_depth_view_refuses_the_pass") else {
        return;
    };
    let target = gpu.attachment(TextureFormat::Rgba8Unorm);
    let depth_view = gpu.attachment(TextureFormat::Depth32Float);
    gpu.device
        .destroy_texture_view(depth_view)
        .expect("the device destroys the depth view it made");

    let refusal = gpu.refusal(&[color(&target, None)], Some(depth(&depth_view)));

    assert_eq!(
        refusal,
        Some(RenderPassError::UnknownDepthView { view: depth_view })
    );
    assert_eq!(gpu.reported_errors(), Vec::<String>::new());
}

/// **An unknown view between known ones refuses the pass at its own index.**
/// No view is paired with another attachment, and nothing panics.
#[test]
fn an_unknown_view_among_colour_attachments_refuses_the_pass_at_its_index() {
    let Some(gpu) =
        Gpu::new("an_unknown_view_among_colour_attachments_refuses_the_pass_at_its_index")
    else {
        return;
    };
    let first = gpu.attachment(TextureFormat::Rgba8Unorm);
    let last = gpu.attachment(TextureFormat::Rgba8Unorm);

    let refusal = gpu.refusal(
        &[
            color(&first, None),
            color(&NEVER_MADE, None),
            color(&last, None),
        ],
        None,
    );

    assert_eq!(
        refusal,
        Some(RenderPassError::UnknownColorView {
            index: 1,
            view: NEVER_MADE
        })
    );
    assert_eq!(gpu.reported_errors(), Vec::<String>::new());
}

/// The shape of a texture [`Gpu::view_of`] makes.
struct Shape {
    format: TextureFormat,
    layers: u32,
    mips: u32,
    samples: SampleCount,
    view: TextureViewDimension,
}

impl Gpu {
    /// Makes a texture of `shape`, usable as an attachment, and returns a view
    /// of all its layers and mips.
    fn view_of(&self, shape: &Shape) -> TextureViewId {
        let texture = self
            .device
            .create_texture(&TextureDescriptor {
                label: Some(Cow::Borrowed("render pass test texture")),
                size: Extent3D {
                    width: SIDE,
                    height: SIDE,
                    depth_or_array_layers: shape.layers,
                },
                mip_level_count: shape.mips,
                sample_count: shape.samples,
                dimension: TextureDimension::D2,
                format: shape.format,
                usage: TextureUsage::RENDER_ATTACHMENT | TextureUsage::TEXTURE_BINDING,
                view_formats: Cow::Borrowed(&[]),
            })
            .expect("the device makes the texture");
        self.device
            .create_texture_view(
                texture,
                &TextureViewDescriptor {
                    label: Some(Cow::Borrowed("render pass test texture view")),
                    format: None,
                    dimension: Some(shape.view),
                    aspect: ImageAspect::All,
                    base_mip_level: 0,
                    mip_level_count: None,
                    base_array_layer: 0,
                    array_layer_count: None,
                },
            )
            .expect("the device makes the texture's view")
    }

    /// Begins, ends and submits a pass over `colors` and `depth`, waits for the
    /// GPU, and returns what wgpu reported.
    fn submitted(
        &self,
        colors: &[RenderPassColorAttachment<'_>],
        depth: Option<RenderPassDepthStencilAttachment<'_>>,
    ) -> Vec<String> {
        let mut encoder = self.device.create_command_encoder(Some("render pass test"));
        drop(
            encoder
                .begin_render_pass(&RenderPassDescriptor {
                    label: Some("render pass test"),
                    color_attachments: colors,
                    depth_stencil_attachment: depth,
                })
                .expect("the pass is begun"),
        );
        let buffer = encoder.finish().expect("the encoder finishes");
        self.device.submit_command_buffer(buffer);
        self.device.wait_for_last_submission();
        self.reported_errors()
    }
}

/// **A refused pass leaves the encoder recording.** A lane whose second pass
/// is refused drops its encoder, but one that records on after a refusal
/// finishes and submits what it recorded, and wgpu reports nothing.
#[test]
fn a_refused_pass_leaves_the_encoder_recording_the_next() {
    let Some(gpu) = Gpu::new("a_refused_pass_leaves_the_encoder_recording_the_next") else {
        return;
    };
    let target = gpu.attachment(TextureFormat::Rgba8Unorm);
    let mut encoder = gpu.device.create_command_encoder(Some("render pass test"));

    let refused = encoder
        .begin_render_pass(&RenderPassDescriptor {
            label: Some("refused"),
            color_attachments: &[color(&NEVER_MADE, None)],
            depth_stencil_attachment: None,
        })
        .map(drop);
    assert!(refused.is_err(), "the pass naming no view is refused");
    drop(
        encoder
            .begin_render_pass(&RenderPassDescriptor {
                label: Some("begun"),
                color_attachments: &[color(&target, None)],
                depth_stencil_attachment: None,
            })
            .expect("the next pass is begun"),
    );
    let buffer = encoder.finish().expect("the encoder finishes");
    gpu.device.submit_command_buffer(buffer);
    gpu.device.wait_for_last_submission();

    assert_eq!(gpu.reported_errors(), Vec::<String>::new());
}

/// **One layer of a depth array is drawn into on its own**, as a shadow
/// atlas's cascade is: the pass draws into that layer, never the whole array.
#[test]
fn a_layer_of_a_depth_array_is_drawn_into_alone() {
    let Some(gpu) = Gpu::new("a_layer_of_a_depth_array_is_drawn_into_alone") else {
        return;
    };
    let target = gpu.attachment(TextureFormat::Rgba8Unorm);
    let atlas = gpu.view_of(&Shape {
        format: TextureFormat::Depth32Float,
        layers: 4,
        mips: 1,
        samples: SampleCount::X1,
        view: TextureViewDimension::D2Array,
    });
    let mut layer = depth(&atlas);
    layer.base_array_layer = 2;

    let errors = gpu.submitted(&[color(&target, None)], Some(layer));

    assert_eq!(errors, Vec::<String>::new());
}

/// **One mip of one cube face is drawn into on its own**, as the IBL
/// prefilter does each roughness.
#[test]
fn a_mip_of_a_cube_face_is_drawn_into_alone() {
    let Some(gpu) = Gpu::new("a_mip_of_a_cube_face_is_drawn_into_alone") else {
        return;
    };
    let cube = gpu.view_of(&Shape {
        format: TextureFormat::Rgba8Unorm,
        layers: 6,
        mips: 2,
        samples: SampleCount::X1,
        view: TextureViewDimension::Cube,
    });
    let mut face = color(&cube, None);
    face.base_array_layer = 3;
    face.base_mip_level = 1;

    let errors = gpu.submitted(&[face], None);

    assert_eq!(errors, Vec::<String>::new());
}

/// **A multisampled pass resolves into its target.**
#[test]
fn a_multisampled_pass_resolves_into_its_target() {
    let Some(gpu) = Gpu::new("a_multisampled_pass_resolves_into_its_target") else {
        return;
    };
    let samples = gpu.view_of(&Shape {
        format: TextureFormat::Rgba8Unorm,
        layers: 1,
        mips: 1,
        samples: SampleCount::X4,
        view: TextureViewDimension::D2,
    });
    let resolved = gpu.attachment(TextureFormat::Rgba8Unorm);

    let errors = gpu.submitted(&[color(&samples, Some(&resolved))], None);

    assert_eq!(errors, Vec::<String>::new());
}

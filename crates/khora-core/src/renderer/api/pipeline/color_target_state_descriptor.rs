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

//! The state of one color target of a render pipeline: its format, blending and write mask.

use crate::khora_bitflags;
use crate::renderer::api::pipeline::{BlendFactor, BlendOperation};
use crate::renderer::api::resource::TextureFormat;

/// Describes the state of a single color target in a render pass.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ColorTargetStateDescriptor {
    /// The texture format of this color target.
    pub format: TextureFormat,
    /// The blending state for this target. If `None`, blending is disabled.
    pub blend: Option<BlendStateDescriptor>,
    /// A bitmask controlling which color channels are written to.
    pub write_mask: ColorWrites,
}

/// Describes the blend state for a single color target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlendStateDescriptor {
    /// The blend equation for the RGB color components.
    pub color: BlendComponentDescriptor,
    /// The blend equation for the Alpha component.
    pub alpha: BlendComponentDescriptor,
}

impl BlendStateDescriptor {
    /// Standard (non-premultiplied) alpha blending:
    /// `dst = src.rgb * src.a + dst.rgb * (1 - src.a)`.
    ///
    /// The alpha channel accumulates coverage with `One / OneMinusSrcAlpha`, so
    /// compositing several transparent layers leaves a correct destination
    /// alpha. This is the curve `AlphaMode::Blend` materials render with; it is
    /// order-dependent, so the caller must draw those materials back-to-front.
    pub const fn alpha_blending() -> Self {
        Self {
            color: BlendComponentDescriptor {
                src_factor: BlendFactor::SrcAlpha,
                dst_factor: BlendFactor::OneMinusSrcAlpha,
                operation: BlendOperation::Add,
            },
            alpha: BlendComponentDescriptor {
                src_factor: BlendFactor::One,
                dst_factor: BlendFactor::OneMinusSrcAlpha,
                operation: BlendOperation::Add,
            },
        }
    }
}

/// Describes a complete blend equation for a single color component (R, G, B, or A).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlendComponentDescriptor {
    /// The blend factor for the source color (from the fragment shader).
    pub src_factor: BlendFactor,
    /// The blend factor for the destination color (already in the framebuffer).
    pub dst_factor: BlendFactor,
    /// The operation to combine the source and destination factors.
    pub operation: BlendOperation,
}

khora_bitflags! {
    /// A bitmask to enable or disable writes to individual color channels.
    pub struct ColorWrites: u8 {
        /// Enable writes to the Red channel.
        const R = 0b0001;
        /// Enable writes to the Green channel.
        const G = 0b0010;
        /// Enable writes to the Blue channel.
        const B = 0b0100;
        /// Enable writes to the Alpha channel.
        const A = 0b1000;
        /// Enable writes to all channels.
        const ALL = Self::R.bits() | Self::G.bits() | Self::B.bits() | Self::A.bits();
    }
}

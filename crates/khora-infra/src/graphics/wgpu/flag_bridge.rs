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

//! Every flag set that crosses into wgpu, declared once as one row per flag.
//!
//! `bridge_flags!` generates each `IntoWgpu` impl and its tests, among them
//! `every_declared_flag_is_bridged`, which fails on a Khora flag no row maps.

use super::conversions::IntoWgpu;
use khora_core::renderer::api::pipeline::state::ColorWrites;
use khora_core::renderer::api::resource::buffer::BufferUsage;
use khora_core::renderer::api::resource::texture::TextureUsage;
use khora_core::renderer::api::util::flags::ShaderStageFlags;

/// Declares how a Khora flag set maps onto a wgpu one, flag by flag.
///
/// Generates the `IntoWgpu` impl and a test module proving the mapping is
/// complete. The Khora type must come from `khora_bitflags!`, which supplies
/// the `ALL_DECLARED` constant the coverage test needs.
///
/// ```ignore
/// bridge_flags! {
///     /// Prose explaining any irregular row.
///     BufferUsage => wgpu::BufferUsages {
///         MAP_READ => MAP_READ,
///         VERTEX   => VERTEX,
///     }
///     tests: buffer_usage_bridge
/// }
/// ```
macro_rules! bridge_flags {
    (
        $(#[$attr:meta])*
        $khora:ty => $wgpu:ty {
            $( $k:ident => $w:ident ),+ $(,)?
        }
        tests: $tests:ident
    ) => {
        $(#[$attr])*
        impl IntoWgpu<$wgpu> for $khora {
            fn into_wgpu(self) -> $wgpu {
                let mut out = <$wgpu>::empty();
                $(
                    if self.contains(<$khora>::$k) {
                        out |= <$wgpu>::$w;
                    }
                )+
                out
            }
        }

        #[cfg(test)]
        mod $tests {
            use super::*;

            /// Each row on its own, so a failure names the flag that broke.
            #[test]
            fn each_flag_translates_to_its_counterpart() {
                $(
                    assert!(
                        <$khora>::$k.into_wgpu().contains(<$wgpu>::$w),
                        concat!(
                            stringify!($k), " did not reach wgpu as ", stringify!($w)
                        )
                    );
                )+
            }

            /// Fails on a flag declared on the Khora type that no row maps.
            #[test]
            fn every_declared_flag_is_bridged() {
                let bridged = <$khora>::EMPTY $( | <$khora>::$k )+;

                assert_eq!(
                    bridged.bits(),
                    <$khora>::ALL_DECLARED.bits(),
                    concat!(
                        "a flag declared on ", stringify!($khora),
                        " is not bridged to ", stringify!($wgpu),
                        " — add a row for it above"
                    )
                );
            }

            /// Translating everything at once yields exactly the union of the
            /// counterparts: nothing lost, nothing invented.
            #[test]
            fn the_whole_set_crosses_intact() {
                let expected = <$wgpu>::empty() $( | <$wgpu>::$w )+;

                assert_eq!(<$khora>::ALL_DECLARED.into_wgpu(), expected);
            }
        }
    };
}

bridge_flags! {
    /// Khora and wgpu number these differently (`VERTEX` is bit 4 here, bit 5
    /// there): a raw bit cast would swap vertex and index buffers.
    BufferUsage => wgpu::BufferUsages {
        MAP_READ => MAP_READ,
        MAP_WRITE => MAP_WRITE,
        COPY_SRC => COPY_SRC,
        COPY_DST => COPY_DST,
        VERTEX => VERTEX,
        INDEX => INDEX,
        UNIFORM => UNIFORM,
        STORAGE => STORAGE,
        INDIRECT => INDIRECT,
        QUERY_RESOLVE => QUERY_RESOLVE,
    }
    tests: buffer_usage_bridge
}

bridge_flags! {
    /// Khora's `DEPTH_STENCIL_ATTACHMENT` folds into wgpu's one `RENDER_ATTACHMENT`.
    ///
    /// Dropping it would make the texture sample-only: a pass using it as a
    /// depth view then fails validation with `TextureViewIsNotRenderable`.
    TextureUsage => wgpu::TextureUsages {
        COPY_SRC => COPY_SRC,
        COPY_DST => COPY_DST,
        TEXTURE_BINDING => TEXTURE_BINDING,
        STORAGE_BINDING => STORAGE_BINDING,
        RENDER_ATTACHMENT => RENDER_ATTACHMENT,
        DEPTH_STENCIL_ATTACHMENT => RENDER_ATTACHMENT,
    }
    tests: texture_usage_bridge
}

bridge_flags! {
    /// Numbered as wgpu's, yet bridged row by row: matching bits are a
    /// coincidence a renumbering on either side would break silently.
    ShaderStageFlags => wgpu::ShaderStages {
        VERTEX => VERTEX,
        FRAGMENT => FRAGMENT,
        COMPUTE => COMPUTE,
    }
    tests: shader_stage_bridge
}

bridge_flags! {
    /// Numbered as wgpu's, bridged row by row for the shader stages' reason.
    ColorWrites => wgpu::ColorWrites {
        R => RED,
        G => GREEN,
        B => BLUE,
        A => ALPHA,
    }
    tests: color_writes_bridge
}

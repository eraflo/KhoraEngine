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

//! Every flag set that crosses into wgpu, declared once.
//!
//! # What went wrong without this
//!
//! Four of Khora's flag sets reach the wgpu backend, and before this file each
//! one crossed differently — with a different way of being wrong:
//!
//! - `BufferUsage` had a hand-written translation that **omitted three flags**.
//!   A buffer asking to be mapped got one that could not be, and the caller
//!   found out at `map_async`.
//! - `create_buffer` **bypassed that translation entirely** and reinterpreted
//!   the raw bits. Khora numbers `VERTEX` at bit 4 and wgpu numbers it at bit 5,
//!   so every empty vertex buffer reached the driver marked as an index buffer.
//!   That was [issue #279], reported from outside the project against a Metal
//!   build, and it survived because nothing named the correspondence in a place
//!   a test could check.
//! - `TextureUsage` was translated correctly, by a free function in `device.rs`
//!   carrying a paragraph explaining the one irregular case — knowledge that
//!   lived beside one call site and helped none of the others.
//! - `ShaderStages` and `ColorWrites` were raw bit casts that happen to be
//!   right, because those two sets happen to be numbered the same as wgpu's.
//!   Correct by coincidence is a thing that stops being correct without anyone
//!   editing it.
//!
//! # The shape
//!
//! [`bridge_flags!`] takes the correspondence as **data** — one row per flag —
//! and generates three things from that one list: the `IntoWgpu` impl, a test
//! per row, and a **coverage test** that fails if the Khora type declares a flag
//! no row mentions.
//!
//! That last one is the point. Omission was the failure mode that lasted
//! longest, precisely because omitted code looks like no code at all. A flag
//! added to `BufferUsage` tomorrow and forgotten here reddens
//! `every_declared_flag_is_bridged` on any machine, with no GPU.
//!
//! Fan-in is expressible: two rows may name the same wgpu flag, which is how
//! Khora's separate `DEPTH_STENCIL_ATTACHMENT` folds into wgpu's single
//! `RENDER_ATTACHMENT`. The irregularity stays visible **as** a row rather than
//! hiding in prose.
//!
//! [issue #279]: https://github.com/eraflo/KhoraEngine/issues/279

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

            /// **The check that catches an omission.** A flag added to the
            /// Khora type and forgotten here leaves a bit no row covers, and
            /// this fails — which is what three silently-dropped `BufferUsage`
            /// flags needed and did not have.
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
    /// Khora and wgpu order these differently — `VERTEX` is bit 4 here and bit
    /// 5 there — so a raw bit cast silently swaps vertex and index buffers.
    /// That was issue #279.
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
    /// **The fan-in row is deliberate.** Khora exposes a separate
    /// `DEPTH_STENCIL_ATTACHMENT` for API clarity; wgpu folds depth and colour
    /// attachments under one `RENDER_ATTACHMENT`. Dropping the depth intent
    /// makes the texture sample-only, and any render pass targeting it as a
    /// depth view then fails validation with `TextureViewIsNotRenderable` —
    /// far from the line that caused it.
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
    /// These happen to be numbered identically to wgpu's, which is exactly why
    /// they went through a raw cast for so long. Correct by coincidence is a
    /// thing that stops being correct without anyone editing it.
    ShaderStageFlags => wgpu::ShaderStages {
        VERTEX => VERTEX,
        FRAGMENT => FRAGMENT,
        COMPUTE => COMPUTE,
    }
    tests: shader_stage_bridge
}

bridge_flags! {
    /// Same story as the shader stages, and the same reason to write it down.
    ColorWrites => wgpu::ColorWrites {
        R => RED,
        G => GREEN,
        B => BLUE,
        A => ALPHA,
    }
    tests: color_writes_bridge
}

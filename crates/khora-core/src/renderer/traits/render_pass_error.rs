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

//! Why a [`CommandEncoder`](super::CommandEncoder) refused to begin a render pass.

use std::fmt;

use crate::renderer::api::resource::TextureViewId;

/// Why a render pass was refused, naming the attachment whose view the device does not hold.
///
/// A refused pass records nothing: no attachment is cleared, loaded or stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RenderPassError {
    /// The colour attachment at `index` names a view the device does not hold.
    UnknownColorView {
        /// The attachment's position in the descriptor's colour attachments.
        index: usize,
        /// The view it names.
        view: TextureViewId,
    },
    /// The resolve target of the colour attachment at `index` names a view the
    /// device does not hold.
    UnknownResolveTarget {
        /// The attachment's position in the descriptor's colour attachments.
        index: usize,
        /// The view its resolve target names.
        view: TextureViewId,
    },
    /// The depth-stencil attachment names a view the device does not hold.
    UnknownDepthView {
        /// The view it names.
        view: TextureViewId,
    },
}

impl fmt::Display for RenderPassError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RenderPassError::UnknownColorView { index, view } => write!(
                f,
                "render pass refused: colour attachment {index} names an unknown view {view:?}"
            ),
            RenderPassError::UnknownResolveTarget { index, view } => write!(
                f,
                "render pass refused: the resolve target of colour attachment {index} names an \
                 unknown view {view:?}"
            ),
            RenderPassError::UnknownDepthView { view } => write!(
                f,
                "render pass refused: the depth attachment names an unknown view {view:?}"
            ),
        }
    }
}

impl std::error::Error for RenderPassError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_refusal_names_its_attachment_and_view() {
        let view = TextureViewId(7);
        let color = RenderPassError::UnknownColorView { index: 2, view }.to_string();
        let resolve = RenderPassError::UnknownResolveTarget { index: 1, view }.to_string();
        let depth = RenderPassError::UnknownDepthView { view }.to_string();

        assert!(color.contains("colour attachment 2"), "{color}");
        assert!(
            resolve.contains("resolve target of colour attachment 1"),
            "{resolve}"
        );
        assert!(depth.contains("depth attachment"), "{depth}");
        for message in [color, resolve, depth] {
            assert!(message.contains("TextureViewId(7)"), "{message}");
        }
    }
}

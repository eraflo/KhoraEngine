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

//! The frame's target with its depth attachment, for a lane that depth-tests.

use super::TargetKey;
use crate::math::Extent2D;
use crate::renderer::api::resource::TextureViewId;

/// The frame's target, read by a lane that depth-tests what it draws: the
/// target with its depth attachment. A frame whose target has no depth is
/// refused at the fetch, as a [`Reason::BadTarget`](crate::lane::guard::Reason).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneTarget {
    /// The colour attachment.
    pub color: TextureViewId,
    /// The depth attachment.
    pub depth: TextureViewId,
    /// Its size in pixels.
    pub size: Extent2D,
    /// Its formats and samples.
    pub key: TargetKey,
}

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

//! The pipelines a lit lane drew with on the current target, one per kind of
//! surface: asked of the `PipelineSystem` once, then kept while the target
//! keeps its key.

use std::collections::HashMap;

use khora_core::lane::io::TargetKey;
use khora_core::renderer::api::pipeline::{PipelineSpec, RenderPipelineId};
use khora_core::renderer::api::shader::ShaderVariantKey;
use khora_core::renderer::error::RenderError;
use khora_core::renderer::traits::PipelineSystem;
use khora_core::renderer::GraphicsDevice;

/// The kind of surface a lit draw is: what its pipeline differs by on one
/// target.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Surface {
    /// The material's texture variant.
    pub variant: ShaderVariantKey,
    /// Whether the material is seen from both sides.
    pub double_sided: bool,
    /// Whether the draw's transform mirrors it.
    pub mirrored: bool,
    /// Whether the material blends.
    pub blend: bool,
}

impl Surface {
    /// An opaque, single-sided, untextured surface: the pipeline a lane warms
    /// when it is prepared.
    pub fn plain() -> Self {
        Self {
            variant: ShaderVariantKey::empty(),
            double_sided: false,
            mirrored: false,
            blend: false,
        }
    }
}

/// The pipelines asked for on one target key.
#[derive(Debug, Default)]
pub(crate) struct VariantPipelines {
    key: Option<TargetKey>,
    built: HashMap<Surface, RenderPipelineId>,
}

impl VariantPipelines {
    /// Keeps what was asked for while the target is `key`; a new key forgets
    /// it all. The pipelines themselves belong to the `PipelineSystem`.
    pub fn retarget(&mut self, key: TargetKey) {
        if self.key != Some(key) {
            self.built.clear();
            self.key = Some(key);
        }
    }

    /// The pipeline for `surface` on the current key: the one kept, else the
    /// one `pipelines` builds from `spec`. `None` before a key is set, or
    /// when no pipeline system is lent and none was kept.
    pub fn get(
        &mut self,
        surface: &Surface,
        device: &dyn GraphicsDevice,
        pipelines: Option<&dyn PipelineSystem>,
        spec: impl FnOnce(&Surface, TargetKey) -> PipelineSpec,
    ) -> Result<Option<RenderPipelineId>, RenderError> {
        let Some(key) = self.key else {
            return Ok(None);
        };
        if let Some(pipeline) = self.built.get(surface) {
            return Ok(Some(*pipeline));
        }
        let Some(pipelines) = pipelines else {
            return Ok(None);
        };
        let pipeline = pipelines.pipeline(device, &spec(surface, key))?;
        self.built.insert(surface.clone(), pipeline);
        Ok(Some(pipeline))
    }

    /// Forgets every pipeline and the key.
    pub fn clear(&mut self) {
        self.built.clear();
        self.key = None;
    }
}

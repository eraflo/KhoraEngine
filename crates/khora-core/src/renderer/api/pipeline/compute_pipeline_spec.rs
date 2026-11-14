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

//! The declarative description of a compute pipeline, and its cache key.

use super::spec::LayoutSpec;
use crate::renderer::api::shader::ShaderVariantKey;

/// Declarative description of a compute pipeline. Mirrors [`PipelineSpec`](super::PipelineSpec) for
/// the compute path (no vertex / depth / color state). The backend resolves the
/// bind-group layouts, compiles the compute shader for the variant, and builds +
/// caches the pipeline keyed by [`ComputePipelineKey`]. Used by Forward+ light
/// culling.
#[derive(Debug, Clone)]
pub struct ComputePipelineSpec {
    /// Debug label.
    pub label: &'static str,
    /// Logical shader module name (e.g. `"khora::pipelines::light_culling"`).
    pub shader: &'static str,
    /// Shader specialization. `empty()` default.
    pub variant: ShaderVariantKey,
    /// Bind-group layouts, in group order (0..N).
    pub bind_group_layouts: Vec<LayoutSpec>,
    /// Compute shader entry point.
    pub entry_point: &'static str,
}

impl ComputePipelineSpec {
    /// The cache key: `(shader, variant)`. For a given shader + variant the
    /// rest of the compute config is fixed.
    pub fn key(&self) -> ComputePipelineKey {
        ComputePipelineKey {
            shader: self.shader,
            variant: self.variant.clone(),
        }
    }
}

/// Cache identity for a compute pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ComputePipelineKey {
    /// Logical shader name.
    pub shader: &'static str,
    /// Shader variant.
    pub variant: ShaderVariantKey,
}

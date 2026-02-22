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

//! Defines data structures for compute pipelines.
//!
//! Compute pipelines are used for general-purpose GPU computing tasks,
//! such as light culling in Forward+ rendering, particle simulations,
//! and other parallel workloads.

use std::borrow::Cow;

use crate::renderer::api::{core::shader::ShaderModuleId, pipeline::layout::PipelineLayoutId};

/// An opaque handle to a compiled compute pipeline state object.
///
/// This ID is returned by [`GraphicsDevice::create_compute_pipeline`] and is used
/// to reference the pipeline when recording compute dispatches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ComputePipelineId(pub u64);

/// A descriptor used to create a [`ComputePipelineId`].
///
/// This struct provides all the necessary information for the `GraphicsDevice` to
/// create a compute pipeline from a compiled shader module.
#[derive(Debug, Clone)]
pub struct ComputePipelineDescriptor<'a> {
    /// An optional debug label for the compute pipeline.
    pub label: Option<Cow<'a, str>>,
    /// The pipeline layout, describing the bind groups used by this pipeline.
    /// If `None`, the layout will be inferred from the shader.
    pub layout: Option<PipelineLayoutId>,
    /// The compiled compute shader module.
    pub shader_module: ShaderModuleId,
    /// The name of the entry point function in the compute shader.
    pub entry_point: Cow<'a, str>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_pipeline_id_creation_and_equality() {
        let id1 = ComputePipelineId(1);
        let id2 = ComputePipelineId(2);
        let id1_again = ComputePipelineId(1);

        assert_eq!(id1, id1_again);
        assert_ne!(id1, id2);
    }

    #[test]
    fn compute_pipeline_id_ordering() {
        let id1 = ComputePipelineId(1);
        let id2 = ComputePipelineId(2);

        assert!(id1 < id2);
    }

    #[test]
    fn compute_pipeline_descriptor_creation() {
        let descriptor = ComputePipelineDescriptor {
            label: Some(Cow::Borrowed("test_compute_pipeline")),
            layout: None,
            shader_module: ShaderModuleId(42),
            entry_point: Cow::Borrowed("cs_main"),
        };

        assert_eq!(descriptor.label.as_deref(), Some("test_compute_pipeline"));
        assert!(descriptor.layout.is_none());
        assert_eq!(descriptor.shader_module, ShaderModuleId(42));
        assert_eq!(descriptor.entry_point, "cs_main");
    }
}

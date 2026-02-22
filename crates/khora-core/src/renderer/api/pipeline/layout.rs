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

//! Pipeline layout descriptors.

use std::borrow::Cow;

/// An opaque handle to a pipeline layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PipelineLayoutId(pub usize);

/// A descriptor for a [`PipelineLayoutId`].
/// Defines the set of resource bindings (e.g., uniform buffers, textures) a pipeline can access.
#[derive(Debug, Clone)]
pub struct PipelineLayoutDescriptor<'a> {
    /// An optional debug label.
    pub label: Option<Cow<'a, str>>,
    /// The bind group layouts used by this pipeline, indexed by set number.
    /// Each bind group layout describes the structure of resources that will be
    /// bound at a specific set index.
    pub bind_group_layouts: &'a [crate::renderer::api::command::BindGroupLayoutId],
}

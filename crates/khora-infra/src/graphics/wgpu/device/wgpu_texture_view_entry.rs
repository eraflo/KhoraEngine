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

//! The record the device keeps for a texture view it created.

use std::sync::Arc;

use khora_core::renderer::api::resource::texture::{self as api_tex};

#[derive(Debug, Clone)]
pub(crate) struct WgpuTextureViewEntry {
    pub(crate) wgpu_view: Arc<wgpu::TextureView>,
    pub(crate) source_texture_id: Option<api_tex::TextureId>,
}

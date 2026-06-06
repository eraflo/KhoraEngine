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

//! Shader decoder: raw `.wgsl` bytes → `CpuShaderSource` (UTF-8).
//!
//! Auto-registered via [`inventory::submit!`] under the canonical
//! `"shader"` slot. Pure CPU work; the hot-reload pump feeds the decoded
//! string to `PipelineSystem::set_overlay_source`.

use anyhow::{Context, Result};
use khora_core::renderer::api::resource::CpuShaderSource;

use crate::asset::{AssetDecoder, DecoderRegistration};

/// Decodes a `.wgsl` shader file into a `CpuShaderSource` (validated UTF-8).
#[derive(Clone, Default)]
pub struct ShaderDecoder;

impl AssetDecoder<CpuShaderSource> for ShaderDecoder {
    fn load(
        &self,
        bytes: &[u8],
    ) -> Result<CpuShaderSource, Box<dyn std::error::Error + Send + Sync + 'static>> {
        let source =
            String::from_utf8(bytes.to_vec()).context("Shader source is not valid UTF-8")?;
        Ok(CpuShaderSource(source))
    }
}

inventory::submit! {
    DecoderRegistration {
        type_name: "shader",
        register: |svc| {
            svc.register_decoder::<CpuShaderSource>("shader", ShaderDecoder);
        },
    }
}

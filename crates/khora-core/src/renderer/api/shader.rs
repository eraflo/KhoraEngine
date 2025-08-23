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

//! Defines data structures for representing shader modules.

use std::borrow::Cow;

/// Represents the source code for a shader module.
///
/// This enum allows for future expansion to other shader languages (like GLSL or SPIR-V)
/// while maintaining a unified API.
#[derive(Debug, Clone)]
pub enum ShaderSourceData<'a> {
    /// The shader source is provided as a WGSL (WebGPU Shading Language) string.
    Wgsl(Cow<'a, str>),
}

/// A descriptor used to create a [`ShaderModuleId`].
///
/// This struct provides all the necessary information for the `GraphicsDevice` to
/// compile a piece of shader code into a usable, backend-specific shader module.
#[derive(Debug, Clone)]
pub struct ShaderModuleDescriptor<'a> {
    /// An optional debug label for the shader module.
    pub label: Option<&'a str>,
    /// The source code of the shader.
    pub source: ShaderSourceData<'a>,
}

/// An opaque handle to a compiled shader module.
///
/// This ID is returned by [`GraphicsDevice::create_shader_module`] and is used to
/// reference the shader in a [`RenderPipelineDescriptor`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ShaderModuleId(pub usize);

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;

    #[test]
    fn shader_module_id_creation_and_equality() {
        let id1 = ShaderModuleId(1);
        let id2 = ShaderModuleId(2);
        let id1_again = ShaderModuleId(1);

        assert_eq!(id1, id1_again);
        assert_ne!(id1, id2);
    }

    #[test]
    fn shader_module_descriptor_creation() {
        let source_code = "fn main() {}";
        let descriptor = ShaderModuleDescriptor {
            label: Some("test_shader"),
            source: ShaderSourceData::Wgsl(Cow::Borrowed(source_code)),
        };

        assert_eq!(descriptor.label, Some("test_shader"));
        let ShaderSourceData::Wgsl(ref cow) = descriptor.source;
        assert_eq!(cow.as_ref(), source_code);
    }
}

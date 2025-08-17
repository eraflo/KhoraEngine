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

use crate::renderer::ShaderStage;
use std::borrow::Cow;

/// Represents the source data for a shader module.
#[derive(Debug, Clone)]
pub enum ShaderSourceData<'a> {
    Wgsl(Cow<'a, str>),
}

/// Describes a shader module to be created by the `GraphicsDevice`.
#[derive(Debug, Clone)]
pub struct ShaderModuleDescriptor<'a> {
    pub label: Option<&'a str>,
    pub source: ShaderSourceData<'a>,
    pub stage: ShaderStage,
    pub entry_point: &'a str,
}

/// An opaque handle representing a compiled shader module.
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
            stage: ShaderStage::Vertex,
            entry_point: "main",
        };

        assert_eq!(descriptor.label, Some("test_shader"));
        assert_eq!(descriptor.entry_point, "main");
        assert_eq!(descriptor.stage, ShaderStage::Vertex);
        let ShaderSourceData::Wgsl(ref cow) = descriptor.source;
        assert_eq!(cow.as_ref(), source_code);
    }
}

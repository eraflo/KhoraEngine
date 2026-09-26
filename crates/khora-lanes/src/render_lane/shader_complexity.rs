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

//! How much lighting work a lit lane's shader does per fragment.

/// Shader complexity levels for resource budgeting and GORNA negotiation.
///
/// This enum represents the relative computational cost of different shader
/// configurations, allowing the rendering system to communicate workload
/// estimates to the resource allocation system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum ShaderComplexity {
    /// No lighting calculations, vertex colors only.
    /// Fastest rendering path.
    Unlit,
    /// Basic Lambertian diffuse + simple specular.
    /// Moderate performance cost.
    #[default]
    SimpleLit,
    /// Full PBR with Cook-Torrance BRDF.
    /// Highest quality, highest cost.
    FullPBR,
}

impl ShaderComplexity {
    /// Returns a cost multiplier for the given complexity level.
    ///
    /// This multiplier is applied to the base rendering cost to estimate
    /// the total GPU workload for different shader configurations.
    pub fn cost_multiplier(&self) -> f32 {
        match self {
            ShaderComplexity::Unlit => 1.0,
            ShaderComplexity::SimpleLit => 1.5,
            ShaderComplexity::FullPBR => 2.5,
        }
    }

    /// Returns a human-readable name for this complexity level.
    pub fn name(&self) -> &'static str {
        match self {
            ShaderComplexity::Unlit => "Unlit",
            ShaderComplexity::SimpleLit => "SimpleLit",
            ShaderComplexity::FullPBR => "FullPBR",
        }
    }
}

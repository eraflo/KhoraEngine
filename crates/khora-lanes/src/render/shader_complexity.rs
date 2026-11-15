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

/// How much lighting work a lit shader does per fragment, as a relative cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum ShaderComplexity {
    /// No lighting, vertex colours only: the cheapest.
    Unlit,
    /// Lambertian diffuse and a simple specular.
    #[default]
    SimpleLit,
    /// Full PBR with a Cook-Torrance BRDF: the dearest.
    FullPBR,
}

impl ShaderComplexity {
    /// Returns the factor this complexity multiplies a base rendering cost by.
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

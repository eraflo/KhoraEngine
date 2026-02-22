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

//! Defines the core traits and material types for the rendering system.

mod alpha_mode;
mod emissive;
mod standard;
mod unlit;
mod wireframe;

pub use alpha_mode::*;
pub use emissive::*;
pub use standard::*;
pub use unlit::*;
pub use wireframe::*;

use std::any::Any;

use super::Asset;

/// Helper trait to allow downcasting `dyn Material` trait objects to their concrete types.
pub trait AsAny {
    /// Returns a reference to the inner value as `&dyn Any`.
    fn as_any(&self) -> &dyn Any;
}

impl<T: Any> AsAny for T {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A trait for types that can be used as a material.
///
/// A material defines the surface properties of an object being rendered,
/// influencing how it interacts with light and determining which shader
/// (`RenderPipeline`) is used to draw it.
pub trait Material: Asset + AsAny {
    /// Returns the base color (albedo or diffuse) of the material.
    /// Default implementation is White.
    fn base_color(&self) -> crate::math::LinearRgba {
        crate::math::LinearRgba::WHITE
    }

    /// Returns the emissive color of the material.
    /// Default implementation is Black.
    fn emissive_color(&self) -> crate::math::LinearRgba {
        crate::math::LinearRgba::BLACK
    }

    /// Returns the specular power or roughness conversion for the material.
    /// Default implementation is 32.0.
    fn specular_power(&self) -> f32 {
        32.0
    }

    /// Returns the ambient color modifier for the material.
    /// Default implementation is (0.1, 0.1, 0.1, 0.0).
    fn ambient_color(&self) -> crate::math::LinearRgba {
        crate::math::LinearRgba::new(0.1, 0.1, 0.1, 0.0)
    }
}

/// This is the key to our type-erased material handle system.
/// We explicitly tell the compiler that a boxed, dynamic Material trait
/// object can itself be treated as a valid Asset. This allows it to be
/// stored inside an AssetHandle.
impl Asset for Box<dyn Material> {}

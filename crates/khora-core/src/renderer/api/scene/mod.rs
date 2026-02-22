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

//! Rendering entities and scene data.

pub mod lighting;
pub mod material_uniforms;
pub mod mesh;
pub mod render_object;

pub use self::lighting::*;
pub use self::material_uniforms::*;
pub use self::mesh::*;
pub use self::render_object::*;

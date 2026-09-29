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

//! Shaders: their modules and sources, the stage they run in, the global defs
//! every shader is composed with, and the variant keys that specialize them.

pub mod defs;
pub mod module;
pub mod source;
pub mod stage;
pub mod variant;

pub use self::defs::ShaderDefs;
pub use self::module::*;
pub use self::source::*;
pub use self::stage::ShaderStage;
pub use self::variant::{ShaderDefScalar, ShaderVariantKey};

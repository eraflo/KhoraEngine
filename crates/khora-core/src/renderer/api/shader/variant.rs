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

//! Shader variants: the specialization defs a pipeline composes its shader with.

/// A scalar shader-def value used to specialize a shader variant. Floats are
/// excluded (naga_oil `#define` is integer/bool only; float consts stay
/// textually injected by the backend).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShaderDefScalar {
    /// A boolean `#ifdef`-style flag.
    Bool(bool),
    /// A signed integer def.
    Int(i32),
    /// An unsigned integer def.
    UInt(u32),
}

/// Identifies a shader specialization: a sorted, deduped set of
/// `(name, value)` defs layered on top of the global `ShaderDefs`.
///
/// `EMPTY` (no defs) is the default variant. The key is `Hash`/`Eq` so it
/// participates in module + pipeline cache keys.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct ShaderVariantKey(Vec<(&'static str, ShaderDefScalar)>);

impl ShaderVariantKey {
    /// The empty variant (no specialization defs).
    pub fn empty() -> Self {
        Self(Vec::new())
    }

    /// Adds (or overwrites) a boolean flag def, keeping the set sorted by name.
    pub fn flag(self, name: &'static str) -> Self {
        self.with(name, ShaderDefScalar::Bool(true))
    }

    /// Adds (or overwrites) a def, keeping the set sorted/deduped by name.
    pub fn with(mut self, name: &'static str, value: ShaderDefScalar) -> Self {
        match self.0.binary_search_by_key(&name, |(n, _)| n) {
            Ok(i) => self.0[i].1 = value,
            Err(i) => self.0.insert(i, (name, value)),
        }
        self
    }

    /// Whether this variant has no defs.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether the named flag is present and set to `Bool(true)`. Used by
    /// variant-dependent layouts (e.g. the material group-2 layout, which
    /// includes only the texture bindings whose `HAS_*` flag is set).
    pub fn has_flag(&self, name: &str) -> bool {
        self.0
            .binary_search_by_key(&name, |(n, _)| n)
            .map(|i| matches!(self.0[i].1, ShaderDefScalar::Bool(true)))
            .unwrap_or(false)
    }

    /// The `(name, value)` defs, sorted by name.
    pub fn defs(&self) -> &[(&'static str, ShaderDefScalar)] {
        &self.0
    }
}

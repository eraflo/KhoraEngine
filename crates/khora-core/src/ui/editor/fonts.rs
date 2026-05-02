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

//! Backend-agnostic font configuration for the editor shell.
//!
//! Apps can supply their own typefaces (proportional + monospace) via the
//! [`FontPack`] type. The shell hands them off to its UI backend (egui, etc.).
//!
//! Fonts are entirely optional: if a [`FontPack`] is never provided, the
//! backend falls back to its built-in defaults.

/// A bundled font, either as a static slice or owned bytes loaded at runtime.
#[derive(Debug, Clone)]
pub enum FontHandle {
    /// Font data bundled into the binary via `include_bytes!`.
    Static(&'static [u8]),
    /// Font data read from disk or fetched at runtime.
    Owned(Vec<u8>),
}

impl FontHandle {
    /// Returns the underlying byte slice.
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Static(s) => s,
            Self::Owned(v) => v.as_slice(),
        }
    }
}

/// A named font face inside a pack. The name is used by the backend to
/// register the font in its registry and (where applicable) to make it
/// addressable by user code.
#[derive(Debug, Clone)]
pub struct NamedFont {
    /// Stable identifier (e.g. `"geist-regular"`, `"geist-mono"`).
    pub name: String,
    /// The raw font bytes.
    pub data: FontHandle,
}

/// A collection of fonts grouped by family.
///
/// The first font in each `Vec` is the *primary* face — the one the backend
/// installs as the default for that family. Subsequent fonts act as
/// fallbacks (e.g. for missing glyph ranges).
#[derive(Debug, Clone, Default)]
pub struct FontPack {
    /// Proportional / sans-serif faces. The first one becomes the default
    /// proportional font.
    pub proportional: Vec<NamedFont>,
    /// Monospaced faces. The first one becomes the default monospace font.
    pub monospace: Vec<NamedFont>,
}

impl FontPack {
    /// Returns `true` if no fonts were specified — backends should leave
    /// their defaults untouched in that case.
    pub fn is_empty(&self) -> bool {
        self.proportional.is_empty() && self.monospace.is_empty()
    }
}

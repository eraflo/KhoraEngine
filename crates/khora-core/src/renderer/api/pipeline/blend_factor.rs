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

//! A factor of a blend equation.

/// A factor in a blend equation, determining how much a source or destination color contributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlendFactor {
    /// The factor is `0.0`.
    Zero,
    /// The factor is `1.0`.
    One,
    /// The factor is the source alpha component (`src.a`).
    SrcAlpha,
    /// The factor is `1.0 - src.a`.
    OneMinusSrcAlpha,
}

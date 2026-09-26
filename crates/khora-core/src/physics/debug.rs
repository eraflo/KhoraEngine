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

//! Debug geometry a backend can draw.

use crate::math::{LinearRgba, Vec3};

/// A simple line for debug rendering.
#[derive(Debug, Clone, Copy)]
pub struct DebugLine {
    /// Start point.
    pub start: Vec3,
    /// End point.
    pub end: Vec3,
    /// Color.
    pub color: LinearRgba,
}

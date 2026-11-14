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

//! The comparison function of depth and stencil tests.

/// The comparison function used for depth and stencil testing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CompareFunction {
    /// The test never passes.
    Never,
    /// The test passes if the new value is less than the existing value.
    Less,
    /// The test passes if the new value is equal to the existing value.
    Equal,
    /// The test passes if the new value is less than or equal to the existing value.
    LessEqual,
    /// The test passes if the new value is greater than the existing value.
    Greater,
    /// The test passes if the new value is not equal to the existing value.
    NotEqual,
    /// The test passes if the new value is greater than or equal to the existing value.
    GreaterEqual,
    /// The test always passes.
    #[default]
    Always,
}

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

//! The operation that combines the two terms of a blend equation.

/// The operation used to combine source and destination colors in a blend equation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlendOperation {
    /// The result is `source + destination`.
    Add,
    /// The result is `source - destination`.
    Subtract,
    /// The result is `destination - source`.
    ReverseSubtract,
    /// The result is `min(source, destination)`.
    Min,
    /// The result is `max(source, destination)`.
    Max,
}

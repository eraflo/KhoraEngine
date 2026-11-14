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

//! An operation on a stencil buffer value.

/// An operation to perform on a stencil buffer value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum StencilOperation {
    /// Keep the existing stencil value.
    #[default]
    Keep,
    /// Set the stencil value to 0.
    Zero,
    /// Replace the stencil value with the reference value.
    Replace,
    /// Bitwise invert the stencil value.
    Invert,
    /// Increment the stencil value, clamping at the maximum value.
    IncrementClamp,
    /// Decrement the stencil value, clamping at 0.
    DecrementClamp,
    /// Increment the stencil value, wrapping to 0 on overflow.
    IncrementWrap,
    /// Decrement the stencil value, wrapping to the maximum value on underflow.
    DecrementWrap,
}

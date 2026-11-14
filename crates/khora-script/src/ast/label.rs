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

//! `Label`: the name an argument is given at a call.

use crate::diagnostics::Span;

/// The name an argument is given at a call: `speed:` in `Move(1, speed: 2)`.
#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    /// The parameter it names.
    pub name: String,
    /// Where the name is written.
    pub span: Span,
}

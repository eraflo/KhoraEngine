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

//! `Import`: a file pulled into a script, optionally under a prefix.

use crate::diagnostics::Span;

/// `import "combat/damage.erg";` or `import "ai/steering.erg" as Steering;`
#[derive(Debug, Clone, PartialEq)]
pub struct Import {
    /// The quoted path, relative to the project's script root.
    pub path: String,
    /// The `as` prefix, when one is given.
    pub alias: Option<String>,
    /// Covers the whole statement, so a circular-import error can point at it.
    pub span: Span,
}

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

//! `EventDecl`: an event a script raises and a handler takes.

use super::decl::Param;
use crate::diagnostics::Span;

/// `event Damaged(int amount, Entity source);` — what a raise names and a
/// handler takes: its name is its identity, its parameters its signature.
#[derive(Debug, Clone, PartialEq)]
pub struct EventDecl {
    /// Event name.
    pub name: String,
    /// Where the name sits.
    pub name_span: Span,
    /// What it carries, in order.
    pub params: Vec<Param>,
    /// The whole declaration.
    pub span: Span,
}

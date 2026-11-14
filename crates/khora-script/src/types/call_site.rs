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

//! `CallSite`: a call as its arguments are checked.

use crate::diagnostics::Span;

/// A call as its arguments are checked: what is written between its
/// parentheses, and where it stands.
pub(crate) struct CallSite<'a> {
    /// The arguments, in the order written.
    pub args: &'a [crate::ast::Expr],
    /// The name each is given — empty when none is.
    pub labels: &'a [Option<crate::ast::Label>],
    /// The whole call.
    pub span: Span,
    /// Its address in the module checked: what its binding is recorded by.
    pub at: usize,
}

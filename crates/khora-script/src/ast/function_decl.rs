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

//! `FunctionDecl`: a free function declaration.

use super::decl::Param;
use super::stmt::Block;
use super::types::TypeRef;
use crate::diagnostics::Span;

/// `fn float Distance(Vec3 a, Vec3 b) { … }`
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDecl {
    /// Function name.
    pub name: String,
    /// Where the name sits.
    pub name_span: Span,
    /// Return type; `void` is a [`TypeRef::Void`].
    pub return_ty: TypeRef,
    /// Parameters.
    pub params: Vec<Param>,
    /// Body.
    pub body: Block,
    /// The whole declaration.
    pub span: Span,
}

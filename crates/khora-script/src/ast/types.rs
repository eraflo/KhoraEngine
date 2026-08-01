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

//! Types as written in the source.
//!
//! Names are kept unresolved on purpose: `int`, `Vec3` and a user's `Health`
//! are all [`TypeRef::Named`] here, and the type checker is what tells them
//! apart. That is what keeps a user type on exactly the same footing as a
//! built-in, rather than a second-class citizen the grammar treats differently.

use crate::diagnostics::Span;

/// A written type.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeRef {
    /// `void`, only valid as a return type.
    Void,
    /// `int`, `Vec3`, `Health` — resolved later, so user types and built-ins
    /// are indistinguishable here.
    Named {
        /// The name.
        name: String,
        /// Its span.
        span: Span,
    },
    /// `T?`
    Optional {
        /// The wrapped type.
        inner: Box<TypeRef>,
        /// Covers `T?`.
        span: Span,
    },
    /// `T[]`
    Array {
        /// The element type.
        element: Box<TypeRef>,
        /// Covers `T[]`.
        span: Span,
    },
    /// `Map<K, V>`
    Generic {
        /// The constructor name.
        name: String,
        /// Its arguments.
        args: Vec<TypeRef>,
        /// The whole type.
        span: Span,
    },
}

impl TypeRef {
    /// Where the type was written.
    pub fn span(&self) -> Span {
        match self {
            Self::Void => Span::empty(0),
            Self::Named { span, .. }
            | Self::Optional { span, .. }
            | Self::Array { span, .. }
            | Self::Generic { span, .. } => *span,
        }
    }

    /// Whether this is an optional, which is what the nullability check keys
    /// off before allowing a value to be used.
    pub fn is_optional(&self) -> bool {
        matches!(self, Self::Optional { .. })
    }
}

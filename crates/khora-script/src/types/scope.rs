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

//! Lexical scopes and narrowing.
//!
//! A stack of frames, innermost first. Ordinary enough — except for what makes
//! the nullability rule usable rather than merely strict.
//!
//! # Narrowing
//!
//! `Entity? target;` cannot be used as an `Entity`. If that were the end of it,
//! every optional would need an explicit unwrap and the feature would be
//! resented. Instead, a null test *narrows* the binding for the branch where it
//! holds:
//!
//! ```text
//! if (var enemy = SeeEnemy())   // enemy: Entity?  → Entity inside the branch
//!     become Chase(enemy);      // no unwrap, no cast, no NullReferenceException
//! ```
//!
//! Narrowing is scoped: [`Scopes::push_narrowed`] shadows the binding for one
//! branch, and popping the frame restores it. So a value narrowed in the `then`
//! branch is optional again in the `else`, which is exactly right — that is
//! where it is known to be absent.

use std::collections::HashMap;

use super::ty::Ty;
use crate::diagnostics::Span;

/// A name bound in some scope.
#[derive(Debug, Clone)]
pub struct Binding {
    /// Its type, after any narrowing.
    pub ty: Ty,
    /// Where it was declared, so a shadowing report can point at both.
    pub span: Span,
}

/// The scope stack.
#[derive(Debug, Default)]
pub struct Scopes {
    frames: Vec<HashMap<String, Binding>>,
}

impl Scopes {
    /// An empty stack with one frame open.
    pub fn new() -> Self {
        Self {
            frames: vec![HashMap::new()],
        }
    }

    /// Opens a frame.
    pub fn push(&mut self) {
        self.frames.push(HashMap::new());
    }

    /// Opens a frame in which `name` is narrowed to `ty`.
    ///
    /// The binding shadows the outer one for the life of the frame, so leaving
    /// the branch restores the optional type without any bookkeeping.
    pub fn push_narrowed(&mut self, name: &str, ty: Ty, span: Span) {
        let mut frame = HashMap::new();
        frame.insert(name.to_owned(), Binding { ty, span });
        self.frames.push(frame);
    }

    /// Closes the innermost frame.
    ///
    /// Never empties the stack: a caller that pops more than it pushed would
    /// otherwise take the global frame with it, and every later lookup would
    /// fail for a reason unrelated to the source.
    pub fn pop(&mut self) {
        if self.frames.len() > 1 {
            self.frames.pop();
        }
    }

    /// Binds `name` in the innermost frame.
    ///
    /// Returns the span of the existing binding when this shadows one *in the
    /// same frame* — a redeclaration, which is an error. Shadowing an outer
    /// frame is allowed and returns `None`.
    pub fn declare(&mut self, name: &str, ty: Ty, span: Span) -> Option<Span> {
        let frame = match self.frames.last_mut() {
            Some(frame) => frame,
            // Cannot happen — `new` opens a frame and `pop` keeps one — but
            // reopening is better than panicking in a compiler.
            None => {
                self.frames.push(HashMap::new());
                self.frames.last_mut()?
            }
        };
        let previous = frame.get(name).map(|binding| binding.span);
        frame.insert(name.to_owned(), Binding { ty, span });
        previous
    }

    /// Looks up `name`, innermost frame first.
    pub fn lookup(&self, name: &str) -> Option<&Binding> {
        self.frames.iter().rev().find_map(|frame| frame.get(name))
    }

    /// The type of `name`, if bound.
    pub fn type_of(&self, name: &str) -> Option<Ty> {
        self.lookup(name).map(|binding| binding.ty.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span() -> Span {
        Span::empty(0)
    }

    #[test]
    fn an_inner_frame_shadows_an_outer_one() {
        let mut scopes = Scopes::new();
        scopes.declare("x", Ty::Int, span());
        scopes.push();
        scopes.declare("x", Ty::Str, span());

        assert_eq!(scopes.type_of("x"), Some(Ty::Str));
        scopes.pop();
        assert_eq!(scopes.type_of("x"), Some(Ty::Int), "the outer one is back");
    }

    /// Redeclaring in the same frame is a mistake and is reported; shadowing an
    /// enclosing frame is ordinary and is not.
    #[test]
    fn redeclaration_is_distinguished_from_shadowing() {
        let mut scopes = Scopes::new();
        assert_eq!(scopes.declare("x", Ty::Int, span()), None);
        assert!(
            scopes.declare("x", Ty::Int, span()).is_some(),
            "same frame: a redeclaration"
        );

        scopes.push();
        assert_eq!(
            scopes.declare("x", Ty::Int, span()),
            None,
            "inner frame: shadowing, which is allowed"
        );
    }

    /// The point of narrowing: an optional becomes present inside the branch
    /// that tested it, and optional again once the branch ends.
    #[test]
    fn narrowing_lasts_exactly_one_branch() {
        let mut scopes = Scopes::new();
        let optional = Ty::Optional(Box::new(Ty::Entity));
        scopes.declare("target", optional.clone(), span());

        scopes.push_narrowed("target", Ty::Entity, span());
        assert_eq!(scopes.type_of("target"), Some(Ty::Entity));

        scopes.pop();
        assert_eq!(
            scopes.type_of("target"),
            Some(optional),
            "outside the branch it is optional again"
        );
    }

    /// Popping more than was pushed must not strand the checker with no frame
    /// at all, or every later lookup fails for a reason unrelated to the code.
    #[test]
    fn the_last_frame_cannot_be_popped() {
        let mut scopes = Scopes::new();
        scopes.declare("x", Ty::Int, span());
        scopes.pop();
        scopes.pop();
        assert_eq!(scopes.type_of("x"), Some(Ty::Int));
    }

    #[test]
    fn an_unbound_name_has_no_type() {
        assert_eq!(Scopes::new().type_of("nope"), None);
    }
}

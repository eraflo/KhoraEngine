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

//! # Component
//!
//! Components are data structures that are attached to entities in the ECS.
//! They are used to store data that is associated with an entity.

use crate::ecs::page::AnyVec;

/// A marker trait for types that can be used as components in the ECS.
///
/// This trait must be implemented for any struct you wish to attach to an entity.
/// The `'static` lifetime ensures that the component type does not contain any
/// non-static references, and `Send + Sync` are required to allow the component
/// data to be safely accessed from multiple threads.
/// `Clone` is required to allow component data to be moved between pages
/// during structural changes (like adding or removing components).
///
/// ## Physical layout (AGDF)
///
/// The three storage hooks below let a component declare *how* its column is
/// physically laid out — the data-layer twin of GORNA's "adapt the HOW, not the
/// WHAT". They all default to the canonical **Array-of-Structures** column
/// (`Vec<Self>`), so existing components are bit-identical to before. A component
/// that opts into a field-split **Structure-of-Arrays** layout (via
/// `#[component(layout = "soa")]`) overrides all three to route through its
/// generated SoA column instead. CRPECS storage stays a `Box<dyn AnyVec>` either
/// way — only the concrete column type behind it changes.
pub trait Component: Clone + 'static + Send + Sync {
    /// Creates this component's empty storage column. Default: an AoS `Vec<Self>`.
    fn make_column() -> Box<dyn AnyVec>
    where
        Self: Sized,
    {
        Box::new(Vec::<Self>::new())
    }

    /// Appends `self` as a new row into its column.
    ///
    /// # Panics
    /// Panics if `column` is not this component's column type — an internal
    /// invariant the registry/bundle paths uphold (columns are keyed by `TypeId`).
    fn push_into_column(self, column: &mut dyn AnyVec)
    where
        Self: Sized,
    {
        column
            .as_any_mut()
            .downcast_mut::<Vec<Self>>()
            .expect("AoS column type mismatch")
            .push(self);
    }

    /// Clones row `src_row` from `src` into `dst` (used when an entity migrates
    /// between pages on a structural change). Both columns are this component's
    /// column type.
    ///
    /// # Panics
    /// Panics on a column type mismatch (same invariant as [`push_into_column`]).
    ///
    /// [`push_into_column`]: Component::push_into_column
    fn copy_row_between(src: &dyn AnyVec, src_row: usize, dst: &mut dyn AnyVec)
    where
        Self: Sized,
    {
        let s = src
            .as_any()
            .downcast_ref::<Vec<Self>>()
            .expect("AoS column type mismatch");
        let dst = dst
            .as_any_mut()
            .downcast_mut::<Vec<Self>>()
            .expect("AoS column type mismatch");
        dst.push(s[src_row].clone());
    }

    /// Reads row `row` of `column` as an owned value — the uniform by-value
    /// access path that works for both AoS (clone the `&T`) and field-SoA
    /// (gather the fields) layouts. Used by `World::clone_component`, the
    /// `Soa<T>` query, and the serialization recipe.
    ///
    /// # Panics
    /// Panics on a column type mismatch.
    fn clone_from_column(column: &dyn AnyVec, row: usize) -> Self
    where
        Self: Sized,
    {
        column
            .as_any()
            .downcast_ref::<Vec<Self>>()
            .expect("AoS column type mismatch")[row]
            .clone()
    }

    /// Overwrites row `row` of `column` with `self` — the uniform by-value write
    /// path for both layouts (AoS assigns the slot; field-SoA scatters into the
    /// lanes). Used by `World::set_component`.
    ///
    /// # Panics
    /// Panics on a column type mismatch.
    fn set_in_column(self, column: &mut dyn AnyVec, row: usize)
    where
        Self: Sized,
    {
        let v = column
            .as_any_mut()
            .downcast_mut::<Vec<Self>>()
            .expect("AoS column type mismatch");
        v[row] = self;
    }
}

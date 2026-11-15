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

//! The `AnyVec` trait: a type-erased component column.

use std::any::Any;

/// A component column whose concrete type its caller does not know.
pub trait AnyVec: Any + Send + Sync {
    /// Casts the trait object to `&dyn Any`.
    fn as_any(&self) -> &dyn Any;

    /// Casts the trait object to `&mut dyn Any`.
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// Performs a `swap_remove` on the underlying column, removing the element at `index`.
    fn swap_remove_any(&mut self, index: usize);

    /// The number of rows the column holds.
    fn len(&self) -> usize;

    /// Whether the column holds no row.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<T: 'static + Send + Sync> AnyVec for Vec<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn swap_remove_any(&mut self, index: usize) {
        self.swap_remove(index);
    }

    fn len(&self) -> usize {
        Vec::len(self)
    }
}

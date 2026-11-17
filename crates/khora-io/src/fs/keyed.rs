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

//! What an entry of a [`RonStore`](super::RonStore) is held by.

/// An entry of a [`RonStore`](super::RonStore): what it is held by.
pub trait Keyed {
    /// What tells two entries apart: a store holds one entry per key.
    type Key: Ord + Clone;

    /// Returns what this entry is held by.
    fn key(&self) -> Self::Key;
}

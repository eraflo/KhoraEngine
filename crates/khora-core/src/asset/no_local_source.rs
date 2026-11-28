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

//! [`NoLocalSource`], the local source of a kind no reference can carry.

use super::{AssetUUID, LocalSource};

/// The local source of a kind no reference can carry: it has no value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoLocalSource {}

impl<A> LocalSource<A> for NoLocalSource {
    const VARIANT: &'static str = "Local";

    fn uuid(&self) -> AssetUUID {
        match *self {}
    }

    fn build(&self) -> A {
        match *self {}
    }
}

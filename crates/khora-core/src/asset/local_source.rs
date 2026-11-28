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

//! [`LocalSource`], the value an asset reference carries itself.

use super::AssetUUID;

/// A value of `A` that an asset reference carries itself, named by its content.
pub trait LocalSource<A>: Clone + Send + Sync + 'static {
    /// The name of the reference's form that carries this source.
    const VARIANT: &'static str;

    /// Returns the identity derived from the source's content.
    fn uuid(&self) -> AssetUUID;

    /// Builds the value the source describes.
    fn build(&self) -> A;
}

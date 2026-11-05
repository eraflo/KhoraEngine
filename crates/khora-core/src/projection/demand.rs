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

//! The kind of thing a projection can be asked for.

/// A kind of demand on a projection: names the items its owners announce.
///
/// A marker: one type per projection, so two demands on two Flows are two
/// [`Interest`](super::Interest)s that cannot be mixed up.
pub trait Demand: Send + Sync + 'static {
    /// What an owner asks the projection for.
    type Item: Ord + Clone + Send + Sync;
}

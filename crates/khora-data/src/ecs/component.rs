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

/// A marker trait for types that can be used as components in the ECS.
///
/// This trait must be implemented for any struct you wish to attach to an entity.
/// The `'static` lifetime ensures that the component type does not contain any
/// non-static references, and `Send + Sync` are required to allow the component
/// data to be safely accessed from multiple threads.
pub trait Component: 'static + Send + Sync {}

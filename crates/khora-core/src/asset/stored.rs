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

//! [`Stored`], the mark of every type the asset system can store and hand out.

/// A marker trait for types that can be managed by the asset system.
///
/// This trait's primary purpose is to categorize a type, making it eligible for
/// use within the engine's asset infrastructure (e.g., in a `Handle<T>`).
///
/// The supertraits enforce critical safety guarantees:
/// - `Send` + `Sync`: The asset type can be safely shared and sent between threads.
///   This is essential for background loading.
/// - `'static`: The asset type does not contain any non-static references, ensuring
///   it can be stored for the lifetime of the application.
///
/// # Examples
///
/// ```
/// use khora_core::asset::Stored;
///
/// // A simple struct representing a texture.
/// struct Texture {
///     // ... fields
/// }
///
/// // By implementing Stored, `Texture` can now be used by the asset system.
/// impl Stored for Texture {}
/// ```
pub trait Stored: Send + Sync + 'static {}

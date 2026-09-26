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

//! This crate provides procedural macros for the Khora Engine.

mod component;
mod ergon_fn;

use proc_macro::TokenStream;

/// Exposes a Rust function to Ergon scripts.
///
/// The annotated function keeps working as ordinary Rust; the macro adds the
/// signature the type checker reads, the trampoline that unpacks a call, and
/// the registration that puts it in the registry. A function is callable from a
/// script by being annotated — there is no second list to keep in step.
///
/// ```ignore
/// #[ergon_fn]
/// fn despawn(context: &mut NativeContext<'_>, entity: EntityId) {
///     context.commands.push(WorldCommand::Despawn { entity });
/// }
///
/// #[ergon_fn(name = "Distance", cost = 4)]
/// fn distance(a: f32, b: f32) -> f32 { (a - b).abs() }
/// ```
///
/// The script name defaults to the function's, in `PascalCase`. `cost` is what
/// one call spends from the frame's fuel budget, and defaults to the price of a
/// single VM instruction — a function that does real work should say so, or the
/// budget stops meaning anything.
///
/// Parameter and return types are translated through
/// `khora_script::native::ScriptType`, so a type alias works, and a crate can
/// expose its own type by implementing that trait rather than by this macro
/// learning about it.
///
/// A leading `&mut NativeContext<'_>` parameter is passed through rather than
/// taken from the script, which is how a function reaches the command buffer
/// and the frame arena.
#[proc_macro_attribute]
pub fn ergon_fn(attr: TokenStream, item: TokenStream) -> TokenStream {
    ergon_fn::ergon_fn(attr, item)
}

/// A derive macro that implements the `khora_data::ecs::Component` trait
/// and generates a serializable mirror struct with `From` conversions.
///
/// For a component like:
/// ```ignore
/// #[derive(Component)]
/// pub struct Camera {
///     pub projection: ProjectionType,
///     pub aspect_ratio: f32,
/// }
/// ```
///
/// This macro generates:
/// - `impl Component for Camera`
/// - `pub struct SerializableCamera { ... }` with `Encode, Decode`
/// - `impl From<Camera> for SerializableCamera`
/// - `impl From<SerializableCamera> for Camera`
///
/// Use `#[component(skip)]` on fields that should not be serialized
/// (e.g., GPU handles). Those fields are filled with `Default::default()`
/// when deserializing back.
#[proc_macro_derive(Component, attributes(component))]
pub fn derive_component(input: TokenStream) -> TokenStream {
    component::derive_component(input)
}

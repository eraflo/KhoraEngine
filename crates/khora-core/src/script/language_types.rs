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

//! The type names Ergon spells itself, which no declaration may take.

/// The engine types a script names: `Vec3`, `Quat`, `Color`, …
///
/// Listed rather than derived: the API surface is a decision. Each name needs
/// a value, a constructor and accessors in the language.
pub const ENGINE_TYPES: [&str; 5] = ["Vec2", "Vec3", "Vec4", "Quat", "Color"];

/// The types the language spells itself beside the engine types: its scalars, its units, `Entity`, `void` and `Map`.
pub const BUILTIN_TYPES: [&str; 9] = [
    "void", "bool", "int", "float", "string", "Duration", "Angle", "Entity", "Map",
];

/// Returns whether the language holds the type named `name` itself, a builtin or an engine type.
pub fn is_language_type(name: &str) -> bool {
    BUILTIN_TYPES.contains(&name) || ENGINE_TYPES.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::{is_language_type, BUILTIN_TYPES, ENGINE_TYPES};
    use crate::script::ErgonType;

    /// **Every type the language spells is held by it**; a struct's name or an asset's is not.
    #[test]
    fn every_type_the_language_spells_is_held_by_it() {
        for name in BUILTIN_TYPES.iter().chain(&ENGINE_TYPES) {
            assert!(is_language_type(name), "{name}");
        }
        for name in ["Loot", "Mesh", "vec3", "Int", ""] {
            assert!(!is_language_type(name), "{name:?}");
        }
    }

    /// **A field's type parses each engine type and each builtin a field holds** as
    /// the language's own, never as a struct of that name.
    #[test]
    fn a_field_type_reads_each_language_type_as_its_own() {
        for name in ENGINE_TYPES {
            assert_eq!(ErgonType::parse(name), Some(ErgonType::Engine(name)));
        }
        for name in BUILTIN_TYPES {
            // No field is `void`, and a `Map` is written with its arguments.
            if matches!(name, "void" | "Map") {
                continue;
            }
            let parsed = ErgonType::parse(name).expect("a builtin parses");
            assert!(
                !matches!(parsed, ErgonType::Struct { .. }),
                "`{name}` reads as a struct"
            );
        }
    }
}

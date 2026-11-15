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

//! What a component looks like to what cannot read Rust: Ergon mirrors, an editor.
//!
//! The derive builds it from the list the `Serializable` mirror is built from, so
//! `#[component(skip)]` holds for both.

/// What a component looks like from outside Rust.
///
/// A marker (no fields) and an opaque shape (an enum) are different answers: a
/// generator must not emit a fieldless mirror for an enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentShape {
    /// A struct — these fields, in declaration order, minus anything
    /// `#[component(skip)]` omits. Empty means a marker component.
    Fields(&'static [FieldSchema]),
    /// Not describable as a list of fields: an enum, one-of rather than all-of.
    Opaque,
}

/// One field of a component, as its author declared it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldSchema {
    /// The field's name. For a tuple struct, its index as a string.
    pub name: &'static str,
    /// The declared Rust type, verbatim — `f32`, `Vec3`, `Option<EntityId>`.
    ///
    /// Kept verbatim so a generator can name the type it could not express.
    pub ty: &'static str,
}

impl ComponentShape {
    /// Returns the fields, or an empty slice for an opaque shape.
    ///
    /// To report on an opaque component, match on the variant instead.
    pub fn fields(&self) -> &'static [FieldSchema] {
        match self {
            Self::Fields(fields) => fields,
            Self::Opaque => &[],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::ComponentRegistration;

    fn registration_named(name: &str) -> &'static ComponentRegistration {
        inventory::iter::<ComponentRegistration>
            .into_iter()
            .find(|r| r.type_name == name)
            .unwrap_or_else(|| panic!("{name} is not registered"))
    }

    fn field_names(name: &str) -> Vec<&'static str> {
        match registration_named(name).shape {
            ComponentShape::Fields(fields) => fields.iter().map(|f| f.name).collect(),
            ComponentShape::Opaque => panic!("{name} reported an opaque shape"),
        }
    }

    /// The schema is the declaration, in order.
    #[test]
    fn a_struct_publishes_its_fields_in_declaration_order() {
        assert_eq!(
            field_names("Transform"),
            ["translation", "rotation", "scale"]
        );
    }

    /// **What `#[component(skip)]` removes, it removes here too.**
    ///
    /// The schema and the `Serializable` mirror are built from one list, so a
    /// field a scene file never sees, a script and an inspector never see.
    #[test]
    fn a_skipped_field_is_absent_from_the_schema() {
        let fields = field_names("RigidBody");

        assert!(
            !fields.contains(&"handle"),
            "`handle` is `#[component(skip)]` — a live solver handle has no \
             business in a mirror; got {fields:?}"
        );
        assert!(fields.contains(&"body_type"), "got {fields:?}");
    }

    /// The declared type is carried verbatim, so a generator that cannot express
    /// one can say which one it could not express.
    #[test]
    fn the_declared_type_is_carried_verbatim() {
        let ComponentShape::Fields(fields) = registration_named("Transform").shape else {
            panic!("Transform is a struct");
        };
        let translation = fields.iter().find(|f| f.name == "translation").unwrap();

        assert_eq!(translation.ty, "Vec3");
    }

    /// **A marker and an enum are not the same answer.** Reporting an enum as
    /// fieldless would let a mirror claim a shape it does not have.
    #[test]
    fn an_enum_is_opaque_rather_than_empty() {
        assert_eq!(
            registration_named("MaterialRef").shape,
            ComponentShape::Opaque
        );
        assert_eq!(registration_named("MeshRef").shape, ComponentShape::Opaque);
    }

    /// A tuple struct addresses its fields by index, which is what an inspector
    /// shows and what a generator writes.
    #[test]
    fn a_tuple_struct_names_its_fields_by_index() {
        assert_eq!(field_names("Name"), ["0"]);
    }

    /// The convenience accessor flattens both cases; the variant is still there
    /// for anything that must tell them apart.
    #[test]
    fn fields_flattens_opaque_to_empty() {
        assert!(ComponentShape::Opaque.fields().is_empty());
        assert_eq!(
            ComponentShape::Fields(&[FieldSchema {
                name: "x",
                ty: "f32"
            }])
            .fields()
            .len(),
            1
        );
    }
}

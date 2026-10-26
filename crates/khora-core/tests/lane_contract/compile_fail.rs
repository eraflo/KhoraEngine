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

//! What the contract refuses to compile, with the message it gives. Each case
//! is a file under `ui/`; its `.stderr` beside it is the expected error.

/// A lane reaches the world only through the Views a Flow published:
/// `InputSource` has no `World`, by field or by method.
#[test]
fn input_source_has_no_world() {
    trybuild::TestCases::new().compile_fail("tests/lane_contract/ui/input_source_has_no_world.rs");
}

/// A field whose type does not fit its attribute fails on that field, with
/// the attribute's own message — not inside the generated `fetch`.
#[test]
fn a_wrongly_annotated_field_points_at_the_field() {
    trybuild::TestCases::new()
        .compile_fail("tests/lane_contract/ui/a_wrongly_annotated_field_points_at_the_field.rs");
}

/// An attribute argument the derive does not know is an error on that
/// argument, never dropped.
#[test]
fn an_unknown_attribute_argument_is_an_error() {
    trybuild::TestCases::new()
        .compile_fail("tests/lane_contract/ui/an_unknown_attribute_argument_is_an_error.rs");
}

/// A field that names no source is an error on that field: the derive could
/// neither declare nor fetch it.
#[test]
fn a_field_without_a_source_is_an_error() {
    trybuild::TestCases::new()
        .compile_fail("tests/lane_contract/ui/a_field_without_a_source_is_an_error.rs");
}

/// The lanes of one agent share their family's outputs: `register_agent!`
/// refuses a lane of another family.
#[test]
fn lanes_of_one_agent_share_outputs() {
    trybuild::TestCases::new()
        .compile_fail("tests/lane_contract/ui/lanes_of_one_agent_share_outputs.rs");
}

// ─── What the derives and `register_agent!` must accept ─────────────────────

/// The generated `declare`/`fetch`/`open`/`commit` bind `d`, `src`, `sink`
/// and `value`: a field with one of those names must not shadow them.
#[test]
fn fields_named_like_the_generated_locals_compile() {
    trybuild::TestCases::new()
        .pass("tests/lane_contract/ui/fields_named_like_the_generated_locals_compile.rs");
}

/// Generic structs, a where-clause, no lifetime, no field: each derives.
#[test]
fn generic_lane_io_structs_derive() {
    trybuild::TestCases::new().pass("tests/lane_contract/ui/generic_lane_io_structs_derive.rs");
}

/// `register_agent!` builds a constant: it can initialise a `static`, and a
/// `&'static` to one, which is what `inventory::submit!` needs.
#[test]
fn register_agent_is_a_constant_expression() {
    trybuild::TestCases::new()
        .pass("tests/lane_contract/ui/register_agent_is_a_constant_expression.rs");
}

/// `#[bus = "…"]` is not an attribute shape the derive knows: it is an error
/// on the attribute, never read as a plain `#[bus]` with its value dropped.
#[test]
fn a_name_value_attribute_is_an_error() {
    trybuild::TestCases::new()
        .compile_fail("tests/lane_contract/ui/a_name_value_attribute_is_an_error.rs");
}

/// `#[bus(validate)]` on a View with no `Validate` impl fails on the field,
/// with `Validate`'s message.
#[test]
fn validate_without_a_check_points_at_the_field() {
    trybuild::TestCases::new()
        .compile_fail("tests/lane_contract/ui/validate_without_a_check_points_at_the_field.rs");
}

/// The generated locals are spelt `__khora_src`, `__khora_sink`, … to stay
/// out of the way of a struct's fields; a field spelt that way is still a
/// field, and must not shadow them either.
#[test]
fn fields_named_like_the_prefixed_locals_compile() {
    trybuild::TestCases::new()
        .pass("tests/lane_contract/ui/fields_named_like_the_prefixed_locals_compile.rs");
}

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

//! A field's default held to its range, at the edges: the same value written
//! in two units, a bound at `f32` precision, a default inside a nested shape,
//! and a field written with no default at all.

use super::{build, refused_at, HOSTS};

/// **The same duration written in two units lies at the bound it equals**:
/// `3000ms` is `3s`, `1500ms` is `1.5s`, `0.7s` is `700ms` — a default
/// written in one unit at a bound written in the other is inside.
#[test]
fn a_default_at_a_bound_written_in_another_unit_lies_inside() {
    for (_, host, _) in HOSTS {
        for field in [
            "[Range(0s, 3s)] Duration cooldown = 3000ms;",
            "[Range(0s, 1.5s)] Duration windup = 1500ms;",
            "[Range(700ms, 2s)] Duration grace = 0.7s;",
        ] {
            build(&host(field));
        }
    }
}

/// **A float bound and a float default are compared as the field stores
/// them**: a default written as the bound's own literal is inside, at the
/// top and at the bottom of the range.
#[test]
fn a_float_default_written_as_its_bound_lies_inside() {
    for (_, host, _) in HOSTS {
        for field in [
            "[Range(0.1, 0.3)] float top = 0.3;",
            "[Range(0.1, 0.3)] float bottom = 0.1;",
            "[Range(0, 1)] float whole = 1.0;",
            "[Range(-0.7, 0.7)] float lean = -0.7;",
            "[Range(0, 10)] int zero = -0;",
        ] {
            build(&host(field));
        }
    }
}

/// **Every element of a default is held to the range, whatever its shape**:
/// an array of optionals holding one value outside, an array of arrays
/// holding one value outside, are refused at the range.
#[test]
fn an_element_outside_the_range_is_refused_in_a_nested_default() {
    for (_, host, _) in HOSTS {
        for field in [
            "[Range(0, 10)] int[][] grid = [[1], [500]];",
            "[Range(0, 10)] int?[] charges = [null, 500];",
        ] {
            refused_at(&host(field), "[Range(0, 10)]", &["default", "outside"]);
        }
    }
}

/// **A number written with no default starts at zero**: a range that leaves
/// zero out is refused at the range, saying the zero and that a new row
/// starts at it — for every kind. An optional (starting at `null`) and an
/// array (starting empty) are inside any range.
#[test]
fn a_field_with_no_default_is_held_to_its_range_at_zero() {
    for (_, host, _) in HOSTS {
        for (attribute, field) in [
            ("[Range(1, 10)]", "int level;"),
            ("[Range(0.5, 2)]", "float speed;"),
            ("[Range(1s, 5s)]", "Duration wait;"),
            ("[Range(10deg, 90deg)]", "Angle turn;"),
        ] {
            refused_at(
                &host(&format!("{attribute} {field}")),
                attribute,
                &["outside", "with no default, a new row starts at"],
            );
        }
        for field in ["[Range(1, 10)] int? level;", "[Range(1, 10)] int[] levels;"] {
            build(&host(field));
        }
    }
}

/// **Rounding never takes in a default that is truly past a bound**: a
/// value the field stores exactly, written whole units beyond the bound, is
/// refused — however large the bound, above the maximum or below a negative
/// minimum. Only the same value written in another unit counts as at it.
#[test]
fn a_default_whole_units_past_a_large_bound_is_refused() {
    for (_, host, _) in HOSTS {
        for (attribute, field) in [
            ("[Range(0, 100000000)]", "float budget = 100000040.0;"),
            ("[Range(0s, 20000000s)]", "Duration lease = 20000008s;"),
            ("[Range(-10000000, 0)]", "float depth = -10000004.0;"),
        ] {
            refused_at(
                &host(&format!("{attribute} {field}")),
                attribute,
                &["default", "outside"],
            );
        }
    }
}

/// **A nested default names the element outside, as an array's does**: the
/// refusal says the default *holds* it, and a nested default with an
/// element that is not a constant is refused for that, not for its range.
#[test]
fn a_nested_default_is_refused_in_an_arrays_words() {
    for (_, host, _) in HOSTS {
        refused_at(
            &host("[Range(0, 10)] int[][] grid = [[1], [500]];"),
            "[Range(0, 10)]",
            &["the default holds `500`", "outside `[Range(0, 10)]`"],
        );
        refused_at(
            &host("[Range(0, 10)] int?[] charges = [null, 500];"),
            "[Range(0, 10)]",
            &["the default holds `500`"],
        );
        let source = host("[Range(0, 10)] int[][] grid = [[500], [other]];");
        let refusal = super::refused_saying(&source, &["constant"]);
        assert!(
            !super::said(&refusal).contains("outside"),
            "{}",
            super::said(&refusal)
        );
    }
}

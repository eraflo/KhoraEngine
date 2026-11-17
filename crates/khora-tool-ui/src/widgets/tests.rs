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

//! The rect and colour helpers every widget lays out with.

use super::*;

#[test]
fn shrink_insets_every_side_and_never_goes_negative() {
    assert_eq!(
        shrink([10.0, 20.0, 100.0, 40.0], 5.0),
        [15.0, 25.0, 90.0, 30.0]
    );
    assert_eq!(shrink([0.0, 0.0, 6.0, 4.0], 5.0), [5.0, 5.0, 0.0, 0.0]);
}

#[test]
fn text_y_centres_a_line_of_the_given_size_in_the_rect() {
    assert_eq!(text_y([0.0, 10.0, 50.0, 30.0], 10.0), 20.0);
    assert_eq!(
        text_y([0.0, 0.0, 50.0, 12.0], 12.0),
        0.0,
        "a line as tall as the rect sits at its top"
    );
}

#[test]
fn on_primary_is_the_theme_inverse_ink() {
    let theme = crate::khora_dark();
    assert_eq!(on_primary(&theme), theme.text_inverse);
}

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

//! `ParleyShaper` turns a string into glyph ids at pen positions, line by
//! line.

use khora_core::ui::text::{ShapeError, ShapeRequest, ShapedGlyph, TextShaper};

use super::{built_in, fresh_id, request, shaped, A, B, H, I, P};
use crate::ui::text::ParleyShaper;

/// The glyph of `id` in `glyphs`; panics if there is none.
fn glyph(glyphs: &[ShapedGlyph], id: u16) -> ShapedGlyph {
    *glyphs
        .iter()
        .find(|g| g.id == id)
        .unwrap_or_else(|| panic!("no glyph {id} in {glyphs:?}"))
}

#[test]
fn the_built_in_font_shapes_without_registering_anything() {
    let shaper = ParleyShaper::new();
    let text = shaper
        .shape(&request("Hello", built_in(), 16.0))
        .expect("the built-in font is registered at construction");
    assert!(!text.glyphs.is_empty());
}

#[test]
fn hello_shapes_into_five_glyphs_left_to_right_on_one_line() {
    let text = shaped(&ParleyShaper::new(), "Hello", 16.0);

    assert_eq!(text.glyphs.len(), 5, "{:?}", text.glyphs);
    assert_eq!(text.lines, 1);
    for pair in text.glyphs.windows(2) {
        assert!(
            pair[1].x > pair[0].x,
            "the pen went back or stayed: {pair:?}"
        );
    }
    assert!(
        text.glyphs[0].x.abs() < 1e-3,
        "the first pen position is the text's left edge, got {}",
        text.glyphs[0].x
    );
    assert!(
        text.size.x > text.glyphs[4].x,
        "the text ends after its last pen position"
    );
    assert!(text.size.y > 0.0);
}

#[test]
fn glyphs_of_one_line_sit_on_one_baseline_inside_the_text() {
    let text = shaped(&ParleyShaper::new(), "Hello", 16.0);
    let baseline = text.glyphs[0].y;
    assert!(
        baseline > 0.0 && baseline < text.size.y,
        "the baseline ({baseline}) lies inside the text's height ({})",
        text.size.y
    );
    for g in &text.glyphs {
        assert_eq!(g.y, baseline, "{g:?} left the line's baseline");
    }
}

#[test]
fn a_letter_shapes_into_the_font_s_own_glyph() {
    let shaper = ParleyShaper::new();
    for (letter, id) in [("H", H), ("a", A), ("b", B), ("i", I), ("p", P)] {
        let text = shaped(&shaper, letter, 16.0);
        let ids: Vec<u16> = text.glyphs.iter().map(|g| g.id).collect();
        assert_eq!(ids, vec![id], "{letter:?} is glyph {id} in the font's cmap");
    }
}

#[test]
fn twice_the_size_is_about_twice_as_wide_and_as_tall() {
    let shaper = ParleyShaper::new();
    let small = shaped(&shaper, "Hello world", 16.0);
    let large = shaped(&shaper, "Hello world", 32.0);

    let width = large.size.x / small.size.x;
    let height = large.size.y / small.size.y;
    assert!((1.95..=2.05).contains(&width), "width ratio {width}");
    assert!((1.9..=2.1).contains(&height), "height ratio {height}");
}

#[test]
fn a_newline_opens_a_second_line_below_the_first() {
    let shaper = ParleyShaper::new();
    let one = shaped(&shaper, "a", 16.0);
    let two = shaped(&shaper, "a\nb", 16.0);

    assert_eq!(two.lines, 2);
    let a = glyph(&two.glyphs, A);
    let b = glyph(&two.glyphs, B);
    assert!(b.y > a.y, "b ({}) is below a ({})", b.y, a.y);
    assert!(b.x.abs() < 1e-3, "b starts its line, got x = {}", b.x);
    assert!(
        (b.y - a.y - one.size.y).abs() < 0.5,
        "the second baseline is one line height ({}) below the first, got {}",
        one.size.y,
        b.y - a.y
    );
    let ratio = two.size.y / one.size.y;
    assert!(
        (1.9..=2.1).contains(&ratio),
        "two lines are twice as tall, got {ratio}"
    );
}

#[test]
fn a_space_advances_the_pen() {
    let shaper = ParleyShaper::new();
    let tight = shaped(&shaper, "ab", 16.0);
    let spaced = shaped(&shaper, "a b", 16.0);

    let gap = glyph(&spaced.glyphs, B).x - glyph(&tight.glyphs, B).x;
    assert!(
        gap > 2.0,
        "a 16 px space moves b right by its advance, got {gap}"
    );
    assert!(spaced.size.x > tight.size.x);
}

#[test]
fn an_empty_text_has_no_glyph_and_no_width() {
    let text = shaped(&ParleyShaper::new(), "", 16.0);
    assert!(text.glyphs.is_empty(), "{:?}", text.glyphs);
    assert_eq!(text.size.x, 0.0);
}

#[test]
fn an_unregistered_font_is_refused() {
    let unknown = fresh_id();
    let refused = ParleyShaper::new().shape(&request("Hello", unknown, 16.0));
    assert_eq!(refused, Err(ShapeError::UnknownFont(unknown)));
}

#[test]
fn a_max_width_narrower_than_the_text_breaks_it_at_spaces() {
    let shaper = ParleyShaper::new();
    let text = "Hello world again";
    let whole = shaped(&shaper, text, 16.0);
    assert_eq!(whole.lines, 1);

    let max_width = whole.size.x * 0.6;
    let wrapped = shaper
        .shape(&ShapeRequest {
            max_width: Some(max_width),
            ..request(text, built_in(), 16.0)
        })
        .expect("the built-in font shapes");

    assert!(wrapped.lines >= 2, "{} line(s)", wrapped.lines);
    assert!(
        wrapped.size.x <= max_width + 0.5,
        "the widest line ({}) fits in {max_width}",
        wrapped.size.x
    );
    let mut baselines: Vec<f32> = wrapped.glyphs.iter().map(|g| g.y).collect();
    baselines.dedup();
    assert!(baselines.len() >= 2, "the glyphs sit on {baselines:?}");
    assert!(wrapped.size.y > whole.size.y);
}

#[test]
fn a_max_width_wider_than_the_text_keeps_it_on_one_line() {
    let shaper = ParleyShaper::new();
    let whole = shaped(&shaper, "Hello world", 16.0);
    let bounded = shaper
        .shape(&ShapeRequest {
            max_width: Some(whole.size.x * 2.0),
            ..request("Hello world", built_in(), 16.0)
        })
        .expect("the built-in font shapes");
    assert_eq!(bounded.lines, 1);
    assert_eq!(bounded.glyphs, whole.glyphs);
}

#[test]
fn shaping_is_deterministic() {
    let shaper = ParleyShaper::new();
    let first = shaped(&shaper, "Hello world\nagain", 18.5);
    let _other = shaped(&shaper, "Something else", 40.0);
    let second = shaped(&shaper, "Hello world\nagain", 18.5);
    let elsewhere = shaped(&ParleyShaper::new(), "Hello world\nagain", 18.5);

    assert_eq!(first, second, "one shaper, twice");
    assert_eq!(first, elsewhere, "two shapers");
}

/// A `max_width` the caller computed from bad data (a NaN layout width) must
/// not panic the frame: Parley asserts on a NaN line width, so the shaper has
/// to keep it from reaching Parley.
#[test]
fn a_max_width_that_is_nan_shapes_without_panicking() {
    let shaper = ParleyShaper::new();
    let shaped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        shaper.shape(&ShapeRequest {
            max_width: Some(f32::NAN),
            ..request("Hello world again", built_in(), 16.0)
        })
    }));
    assert!(
        shaped.is_ok(),
        "shaping with max_width = NaN panicked instead of shaping or refusing"
    );
}

/// A size that is not a positive, finite number of pixels draws nothing: no
/// glyph to key, rasterise and place, and no NaN in what comes back. (Today a
/// NaN or infinite size gives NaN pen positions on two lines, and a negative
/// one runs the pen leftwards above the text's top edge.)
#[test]
fn a_size_that_is_not_a_positive_finite_number_shapes_into_no_glyph() {
    let shaper = ParleyShaper::new();
    for size_px in [0.0, -16.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let Ok(text) = shaper.shape(&request("Hello world", built_in(), size_px)) else {
            continue;
        };
        assert!(
            text.glyphs.is_empty(),
            "size {size_px} px shaped into {:?}",
            text.glyphs
        );
        assert!(
            text.size.x.is_finite() && text.size.y.is_finite(),
            "size {size_px} px gave a {:?} box",
            text.size
        );
    }
}

/// A tab is whitespace: it advances the pen and draws nothing. The default
/// font has no glyph for U+0009, so the tab must not come back as the font's
/// `.notdef` (glyph 0), which rasterises into a visible box.
#[test]
fn a_tab_shapes_into_no_notdef_glyph() {
    let text = shaped(&ParleyShaper::new(), "a\tb", 16.0);
    assert!(
        text.glyphs.iter().all(|g| g.id != 0),
        "the tab became .notdef: {:?}",
        text.glyphs
    );
    let tight = shaped(&ParleyShaper::new(), "ab", 16.0);
    assert!(
        glyph(&text.glyphs, B).x > glyph(&tight.glyphs, B).x,
        "the tab advances the pen"
    );
}

/// A line break written `\r\n` (a file saved on Windows) opens one line, not
/// two, and neither control character comes back as a glyph to draw.
#[test]
fn a_crlf_opens_one_line_and_draws_only_the_letters() {
    let shaper = ParleyShaper::new();
    let lf = shaped(&shaper, "a\nb", 16.0);
    let crlf = shaped(&shaper, "a\r\nb", 16.0);

    assert_eq!(crlf.lines, 2);
    let ids: Vec<u16> = crlf.glyphs.iter().map(|g| g.id).collect();
    assert_eq!(ids, vec![A, B], "only a and b are drawn");
    assert_eq!(crlf, lf, "\r\n lays out as \n");
}

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

//! Registering fonts with both backends: the family read from the file, bytes
//! that are no font, and an id registered again (a hot reload).

use std::sync::Arc;

use khora_core::ui::text::{
    FontError, FontFamily, GlyphRasterizer, ShapeError, ShapedText, TextShaper,
};

use super::{built_in, default_font, fresh_id, key, plex_mono, request, shaped, H};
use crate::ui::text::{ParleyShaper, SwashRasterizer};

fn shape_in(shaper: &ParleyShaper, font: khora_core::asset::AssetUUID, text: &str) -> ShapedText {
    shaper
        .shape(&request(text, font, 16.0))
        .unwrap_or_else(|e| panic!("{text:?} was refused: {e}"))
}

#[test]
fn registering_the_default_font_reads_its_family_from_the_file() {
    let family = ParleyShaper::new()
        .register_font(fresh_id(), default_font())
        .expect("the default font is a readable font");
    // `InterVariable.ttf` names its family "Inter Variable" (name id 1; it has
    // no typographic family, name id 16).
    assert_eq!(
        family,
        FontFamily {
            name: "Inter Variable".to_owned()
        }
    );
}

#[test]
fn registering_another_font_reads_that_font_s_family() {
    let family = ParleyShaper::new()
        .register_font(fresh_id(), plex_mono())
        .expect("IBM Plex Mono is a readable font");
    assert_eq!(family.name, "IBM Plex Mono");
}

#[test]
fn the_default_font_under_a_new_id_shapes_like_the_built_in() {
    let shaper = ParleyShaper::new();
    let id = fresh_id();
    shaper
        .register_font(id, default_font())
        .expect("the default font registers");
    assert_eq!(
        shape_in(&shaper, id, "Hello world"),
        shaped(&shaper, "Hello world", 16.0)
    );
}

#[test]
fn bytes_that_are_no_font_are_refused_by_the_shaper() {
    let shaper = ParleyShaper::new();
    let garbage: Arc<[u8]> = Arc::from(&b"this is not a font file at all"[..]);
    assert_eq!(
        shaper.register_font(fresh_id(), garbage),
        Err(FontError::Unreadable)
    );
    assert_eq!(
        shaper.register_font(fresh_id(), Arc::from(&[][..])),
        Err(FontError::Unreadable),
        "no bytes at all"
    );
}

#[test]
fn a_refused_font_leaves_its_id_unknown() {
    let shaper = ParleyShaper::new();
    let id = fresh_id();
    let _ = shaper.register_font(id, Arc::from(&b"garbage"[..]));
    assert_eq!(
        shaper.shape(&request("Hello", id, 16.0)),
        Err(ShapeError::UnknownFont(id))
    );
}

#[test]
fn bytes_that_are_no_font_are_refused_by_the_rasterizer() {
    let rasterizer = SwashRasterizer::new();
    let id = fresh_id();
    assert_eq!(
        rasterizer.register_font(id, Arc::from(&b"this is not a font file at all"[..])),
        Err(FontError::Unreadable)
    );
    assert_eq!(
        rasterizer.register_font(fresh_id(), Arc::from(&[][..])),
        Err(FontError::Unreadable),
        "no bytes at all"
    );
    assert_eq!(rasterizer.rasterize(&key(id, H, 16.0, 0)), None);
}

#[test]
fn registering_an_id_again_replaces_its_font_in_the_shaper() {
    let shaper = ParleyShaper::new();
    let id = fresh_id();

    shaper
        .register_font(id, default_font())
        .expect("Inter registers");
    let narrow = shape_in(&shaper, id, "iiii").size.x;
    let wide = shape_in(&shaper, id, "MMMM").size.x;
    assert!(
        wide > narrow * 1.5,
        "Inter is proportional: {narrow} vs {wide}"
    );

    let family = shaper
        .register_font(id, plex_mono())
        .expect("IBM Plex Mono registers over Inter");
    assert_eq!(family.name, "IBM Plex Mono");
    let narrow = shape_in(&shaper, id, "iiii").size.x;
    let wide = shape_in(&shaper, id, "MMMM").size.x;
    assert!(
        (wide - narrow).abs() < 0.01,
        "the id now shapes with the monospaced font: {narrow} vs {wide}"
    );
}

#[test]
fn registering_another_font_leaves_the_built_in_alone() {
    let shaper = ParleyShaper::new();
    let before = shaped(&shaper, "Hello world", 16.0);
    let id = fresh_id();
    shaper
        .register_font(id, plex_mono())
        .expect("IBM Plex Mono registers");

    assert_eq!(shaped(&shaper, "Hello world", 16.0), before);
    assert_ne!(shape_in(&shaper, id, "Hello world"), before);
}

#[test]
fn the_rasterizer_s_built_in_font_is_the_default_font() {
    let rasterizer = SwashRasterizer::new();
    let id = fresh_id();
    rasterizer
        .register_font(id, default_font())
        .expect("the default font registers");

    let built = rasterizer.rasterize(&key(built_in(), H, 24.0, 0));
    assert!(
        built.is_some(),
        "the built-in font is registered at construction"
    );
    assert_eq!(rasterizer.rasterize(&key(id, H, 24.0, 0)), built);
}

#[test]
fn registering_an_id_again_replaces_its_font_in_the_rasterizer() {
    let rasterizer = SwashRasterizer::new();
    let id = fresh_id();
    rasterizer
        .register_font(id, default_font())
        .expect("Inter registers");
    let inter = rasterizer.rasterize(&key(id, H, 24.0, 0));
    assert!(inter.is_some());

    rasterizer
        .register_font(id, plex_mono())
        .expect("IBM Plex Mono registers over Inter");
    assert_ne!(
        rasterizer.rasterize(&key(id, H, 24.0, 0)),
        inter,
        "glyph {H} of the id is now IBM Plex Mono's, not Inter's"
    );
    assert_eq!(
        rasterizer.rasterize(&key(built_in(), H, 24.0, 0)),
        inter,
        "the built-in font is still Inter"
    );
}

#[test]
fn default_built_backends_have_the_built_in_font() {
    let shaper = ParleyShaper::default();
    assert!(shaper.shape(&request("Hello", built_in(), 16.0)).is_ok());
    let rasterizer = SwashRasterizer::default();
    assert!(rasterizer.rasterize(&key(built_in(), H, 16.0, 0)).is_some());
}

/// Archivo Regular (weight 400) and SemiBold (weight 600): one family, two
/// weights, the same glyph order.
const ARCHIVO_REGULAR: &[u8] =
    include_bytes!("../../../../../khora-tool-ui/assets/fonts/Archivo-Regular.ttf");
const ARCHIVO_SEMIBOLD: &[u8] =
    include_bytes!("../../../../../khora-tool-ui/assets/fonts/Archivo-SemiBold.ttf");

/// A font collection (`.ttc`) holding `faces` in this order: the `ttcf`
/// header, then each face's file with its table offsets moved to where the
/// face now starts.
fn collection(faces: &[&[u8]]) -> Arc<[u8]> {
    fn be32(bytes: &[u8], at: usize) -> u32 {
        u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
    }
    let mut out = vec![0_u8; 12 + 4 * faces.len()];
    out[..4].copy_from_slice(b"ttcf");
    out[4..8].copy_from_slice(&0x0001_0000_u32.to_be_bytes());
    out[8..12].copy_from_slice(&(faces.len() as u32).to_be_bytes());
    for (i, face) in faces.iter().enumerate() {
        while !out.len().is_multiple_of(4) {
            out.push(0);
        }
        let base = out.len() as u32;
        out[12 + 4 * i..16 + 4 * i].copy_from_slice(&base.to_be_bytes());
        let mut face = face.to_vec();
        let tables = usize::from(u16::from_be_bytes([face[4], face[5]]));
        for table in 0..tables {
            let offset = 12 + 16 * table + 8;
            let moved = be32(&face, offset) + base;
            face[offset..offset + 4].copy_from_slice(&moved.to_be_bytes());
        }
        out.extend_from_slice(&face);
    }
    Arc::from(out)
}

/// A collection registers as one font: the shaper and the rasteriser must use
/// the **same face** of it, or the glyphs are placed at one face's advances
/// and drawn with another face's outlines. Here the collection holds
/// SemiBold then Regular; the shaper picks Regular (the weight it asks
/// for), the rasteriser always takes the file's first face, SemiBold.
#[test]
fn both_backends_use_the_same_face_of_a_font_collection() {
    let shaper = ParleyShaper::new();
    let rasterizer = SwashRasterizer::new();
    let (regular, semibold, both) = (fresh_id(), fresh_id(), fresh_id());
    for (id, data) in [
        (regular, Arc::<[u8]>::from(ARCHIVO_REGULAR)),
        (semibold, Arc::<[u8]>::from(ARCHIVO_SEMIBOLD)),
        (both, collection(&[ARCHIVO_SEMIBOLD, ARCHIVO_REGULAR])),
    ] {
        shaper
            .register_font(id, Arc::clone(&data))
            .expect("the shaper reads the font");
        rasterizer
            .register_font(id, data)
            .expect("the rasteriser reads the font");
    }

    let shaped_with = |id| {
        shaper
            .shape(&request("Hello", id, 32.0))
            .expect("the font shapes")
    };
    let face = if shaped_with(both) == shaped_with(regular) {
        regular
    } else {
        assert_eq!(shaped_with(both), shaped_with(semibold));
        semibold
    };
    let h = shaped_with(both).glyphs[0].id;
    // An image as its size and its total coverage: equal for one face, apart
    // for two weights.
    let drawn = |id| {
        rasterizer.rasterize(&key(id, h, 32.0, 0)).map(|i| {
            (
                i.width,
                i.height,
                i.pixels.iter().map(|&p| u32::from(p)).sum::<u32>(),
            )
        })
    };
    assert_eq!(
        drawn(both),
        drawn(face),
        "the shaper laid the collection out with {}, the rasteriser drew another face",
        if face == regular {
            "Regular"
        } else {
            "SemiBold"
        }
    );
}

/// The two backends agree on what is a font: a file cut short (a download
/// that stopped, a partial write) that the shaper refuses as unreadable is
/// refused by the rasteriser too, instead of registering a font that draws
/// nothing.
#[test]
fn a_truncated_font_is_refused_by_the_rasterizer_as_by_the_shaper() {
    let shaper = ParleyShaper::new();
    let rasterizer = SwashRasterizer::new();
    for length in [1_000, 10_000, 100_000] {
        let cut: Arc<[u8]> = Arc::from(&super::DEFAULT_FONT[..length]);
        assert_eq!(
            shaper.register_font(fresh_id(), Arc::clone(&cut)),
            Err(FontError::Unreadable),
            "the shaper refuses the first {length} bytes"
        );
        assert_eq!(
            rasterizer.register_font(fresh_id(), cut),
            Err(FontError::Unreadable),
            "the rasteriser accepted the first {length} bytes"
        );
    }
}

/// Registering an id again (a hot reload) lets go of the replaced file: after
/// shaping and rasterising with it, neither backend keeps its bytes alive.
#[test]
fn replacing_a_font_lets_go_of_the_old_file() {
    let shaper = ParleyShaper::new();
    let rasterizer = SwashRasterizer::new();
    let id = fresh_id();
    let old = default_font();

    shaper
        .register_font(id, Arc::clone(&old))
        .expect("Inter registers with the shaper");
    rasterizer
        .register_font(id, Arc::clone(&old))
        .expect("Inter registers with the rasteriser");
    let _ = shape_in(&shaper, id, "Hello world");
    let _ = rasterizer.rasterize(&key(id, H, 24.0, 0));
    assert!(Arc::strong_count(&old) > 1, "the backends hold the font");

    shaper
        .register_font(id, plex_mono())
        .expect("IBM Plex Mono registers over Inter");
    rasterizer
        .register_font(id, plex_mono())
        .expect("IBM Plex Mono registers over Inter");
    let _ = shape_in(&shaper, id, "Hello world");
    let _ = rasterizer.rasterize(&key(id, H, 24.0, 0));
    assert_eq!(
        Arc::strong_count(&old),
        1,
        "a backend still holds the replaced font"
    );
}

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

//! The backends as the runtime holds them: `Arc<dyn TextShaper>` and
//! `Arc<dyn GlyphRasterizer>`, shared between threads.

use std::sync::Arc;

use khora_core::ui::text::{GlyphRasterizer, TextShaper};

use super::{built_in, default_font, fresh_id, key, request, shaped, H};
use crate::ui::text::{ParleyShaper, SwashRasterizer};

fn is_send_sync<T: Send + Sync + ?Sized>() {}

#[test]
fn the_shaper_works_as_a_shared_trait_object() {
    is_send_sync::<Arc<dyn TextShaper>>();
    let shaper: Arc<dyn TextShaper> = Arc::new(ParleyShaper::new());

    let through_the_trait = shaper
        .shape(&request("Hello", built_in(), 16.0))
        .expect("the built-in font shapes through the trait");
    assert_eq!(
        through_the_trait,
        shaped(&ParleyShaper::new(), "Hello", 16.0)
    );

    let id = fresh_id();
    let family = shaper
        .register_font(id, default_font())
        .expect("a font registers through the trait");
    assert!(!family.name.is_empty());
    assert!(shaper.shape(&request("Hello", id, 16.0)).is_ok());
}

#[test]
fn the_rasterizer_works_as_a_shared_trait_object() {
    is_send_sync::<Arc<dyn GlyphRasterizer>>();
    let rasterizer: Arc<dyn GlyphRasterizer> = Arc::new(SwashRasterizer::new());

    let through_the_trait = rasterizer.rasterize(&key(built_in(), H, 16.0, 0));
    assert!(through_the_trait.is_some());
    assert_eq!(
        through_the_trait,
        SwashRasterizer::new().rasterize(&key(built_in(), H, 16.0, 0))
    );

    let id = fresh_id();
    rasterizer
        .register_font(id, default_font())
        .expect("a font registers through the trait");
    assert!(rasterizer.rasterize(&key(id, H, 16.0, 0)).is_some());
}

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

use super::{document, frame_count, Fill, SCENES, SOURCES};
use std::path::Path;

fn fill() -> Fill {
    Fill {
        frames: [36, 24, 30, 48],
        valley_entities: "5,838".to_owned(),
    }
}

/// The book's pages are reached from the site root as they are, and the
/// landing's images from its own folder.
#[test]
fn links_point_into_the_book_and_images_into_the_landing_folder() {
    let page = document(
        r#"<a href="{root}home.html">Khora</a><img src="{img}chess/frame_000.jpg">"#,
        &fill(),
    );
    assert!(page.contains(r#"<a href="home.html">Khora</a>"#), "{page}");
    assert!(
        page.contains(r#"src="landing/images/chess/frame_000.jpg""#),
        "{page}"
    );
}

/// Every count the page shows or plays is filled in, the valley's last frame
/// with its three digits.
#[test]
fn the_counts_are_filled_in() {
    let page = document(
        "{chess_frames} {valley_frames} {fireflies_frames} {physics_frames} {valley_last} {valley_entities}",
        &fill(),
    );
    assert!(page.contains("36 24 30 48 023 5,838"), "{page}");
}

/// The per-frame pattern the reels fill in themselves is left alone.
#[test]
fn the_reel_pattern_is_left_for_the_script() {
    let page = document(r#"data-pattern="{img}valley/frame_{n}.jpg""#, &fill());
    assert!(
        page.contains(r#"data-pattern="landing/images/valley/frame_{n}.jpg""#),
        "{page}"
    );
}

/// A page is a whole document: charset, viewport, its stylesheet and script.
#[test]
fn the_page_is_a_whole_document() {
    let page = document("<main></main>", &fill());
    assert!(page.starts_with("<!doctype html>"));
    for needed in [
        r#"<meta charset="utf-8">"#,
        r#"name="viewport""#,
        r#"href="landing/landing.css""#,
        r#"src="landing/landing.js""#,
        "<main></main>",
    ] {
        assert!(page.contains(needed), "missing {needed}");
    }
}

/// The page shipped in the repository has a placeholder for nothing the
/// build does not fill.
#[test]
fn the_shipped_page_leaves_no_placeholder() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let body = std::fs::read_to_string(root.join(SOURCES).join("page.html"))
        .expect("docs/landing/page.html is readable");
    let page = document(&body, &fill());
    let leftover: Vec<&str> = page
        .match_indices('{')
        .filter_map(|(at, _)| page[at..].find('}').map(|end| &page[at..=at + end]))
        .filter(|token| {
            token.len() < 24
                && token[1..token.len() - 1]
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '_')
        })
        .filter(|token| *token != "{n}")
        .collect();
    assert!(leftover.is_empty(), "unfilled: {leftover:?}");
}

/// Every scene the page plays has its frames in the repository.
#[test]
fn every_scene_has_frames() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    for scene in SCENES {
        let dir = root.join(SOURCES).join("images").join(scene);
        assert!(
            frame_count(&dir) > 1,
            "{scene} has no frames in {}",
            dir.display()
        );
    }
}

/// A missing folder holds no frames, rather than failing the count.
#[test]
fn a_missing_folder_has_no_frames() {
    assert_eq!(frame_count(Path::new("no/such/folder")), 0);
}

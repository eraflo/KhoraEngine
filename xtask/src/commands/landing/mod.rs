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

//! `cargo xtask landing <site>`: the landing page, written at the root of a
//! built book.
//!
//! The book's own first page stays `home.html`; the landing takes the site's
//! `index.html`, the page a visitor lands on. Its sources live in
//! `docs/landing/`: `page.html` (the page's body, with `{root}` for the
//! book's pages and `{img}` for its images), `landing.css`, `landing.js`,
//! `valley-entities.txt`, and one folder of rendered frames per scene under
//! `images/`. Run after `mdbook build`.

use crate::term::*;
use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests;

/// Where the landing's sources are, from the repository root.
const SOURCES: &str = "docs/landing";

/// The folder of the site the landing's own files go to.
const OUT_DIR: &str = "landing";

/// The scenes whose frames the page plays, each a folder under `images/`.
const SCENES: [&str; 4] = ["chess", "valley", "fireflies", "physics"];

/// What a page needs filled in: the frames each scene has, and the number of
/// entities the valley holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fill {
    /// Frames per scene, in [`SCENES`] order.
    pub frames: [usize; 4],
    /// The valley's entity count, as the page shows it (`5,838`).
    pub valley_entities: String,
}

/// Writes the landing into `site`, a built book: `site/index.html`, and its
/// stylesheet, script and images under `site/landing/`.
pub fn build(site: &Path) -> Result<()> {
    print_task_start("Writing the landing page", ROCKET, MAGENTA);
    if !site.join("home.html").is_file() {
        bail!(
            "{} holds no built book (no home.html): run `mdbook build docs` first",
            site.display()
        );
    }
    let sources = PathBuf::from(SOURCES);
    let page = fs::read_to_string(sources.join("page.html"))
        .with_context(|| format!("reading {}/page.html", SOURCES))?;
    let fill = Fill {
        frames: SCENES.map(|scene| frame_count(&sources.join("images").join(scene))),
        valley_entities: fs::read_to_string(sources.join("valley-entities.txt"))
            .with_context(|| format!("reading {}/valley-entities.txt", SOURCES))?
            .trim()
            .to_owned(),
    };
    if let Some(scene) = SCENES.iter().zip(fill.frames).find(|(_, n)| *n == 0) {
        bail!("no frames for the {} scene in {}/images", scene.0, SOURCES);
    }

    let out = site.join(OUT_DIR);
    fs::create_dir_all(&out)?;
    for file in ["landing.css", "landing.js"] {
        fs::copy(sources.join(file), out.join(file)).with_context(|| format!("copying {file}"))?;
    }
    copy_dir(&sources.join("images"), &out.join("images"))?;
    fs::write(site.join("index.html"), document(&page, &fill)).context("writing index.html")?;
    print_success(&format!(
        "Landing written to {}",
        site.join("index.html").display()
    ));
    Ok(())
}

/// The whole page: the shell (fonts, favicon, stylesheet) around `body`, its
/// placeholders filled. The landing sits at the book's root, so the book's
/// pages are reached from there as they are, and its files under `landing/`.
pub fn document(body: &str, fill: &Fill) -> String {
    let [chess, valley, fireflies, physics] = fill.frames;
    let body = body
        .replace("{root}", "")
        .replace("{img}", &format!("{OUT_DIR}/images/"))
        .replace("{chess_frames}", &chess.to_string())
        .replace("{valley_frames}", &valley.to_string())
        .replace("{fireflies_frames}", &fireflies.to_string())
        .replace("{physics_frames}", &physics.to_string())
        .replace("{valley_last}", &format!("{:03}", valley.saturating_sub(1)))
        .replace("{valley_entities}", &fill.valley_entities);
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Khora Engine</title>
<meta name="description" content="Khora is a free, open-source game engine written in Rust. Sixty times a second, its parts agree on how to share the frame, so your game keeps its pace on any machine.">
<meta name="color-scheme" content="dark">
<link rel="icon" href="favicon.svg">
<link rel="shortcut icon" href="favicon.png">
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Archivo:ital,wdth,wght@0,62..125,100..900;1,62..125,100..900&family=IBM+Plex+Mono:ital,wght@0,400;0,500;0,600;1,400&display=swap">
<link rel="stylesheet" href="{OUT_DIR}/landing.css">
<style>html, body {{ margin: 0; background: oklch(0.13 0.02 265); }}</style>
</head>
<body>
{body}
<script src="{OUT_DIR}/landing.js"></script>
</body>
</html>
"#
    )
}

/// The `frame_NNN.jpg` files in `dir`; zero when the folder is missing.
pub fn frame_count(dir: &Path) -> usize {
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .filter(|entry| {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    name.starts_with("frame_") && name.ends_with(".jpg")
                })
                .count()
        })
        .unwrap_or(0)
}

/// Copies the files of `from` into `to`, folders included.
fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from).with_context(|| format!("reading {}", from.display()))? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)
                .with_context(|| format!("copying {}", entry.path().display()))?;
        }
    }
    Ok(())
}

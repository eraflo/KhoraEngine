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

//! Editor brand fonts — Geist & Geist Mono loader.
//!
//! Loads Khora's brand fonts at startup through the same I/O layer as project
//! assets ([`khora_sdk::AssetIo`] / [`khora_sdk::FileLoader`]). Today the
//! loader hits the disk; once the editor ships as a packed binary we can swap
//! `FileLoader` for `PackLoader` without touching the call sites.
//!
//! If the font files are missing, returns an empty [`FontPack`] — the shell
//! will silently keep its built-in defaults. No build-time check, no recompile
//! required when adding the fonts later.
//!
//! Expected layout (relative to the loader root):
//! ```text
//! fonts/
//!   Geist-Regular.ttf
//!   Geist-Medium.ttf
//!   Geist-SemiBold.ttf
//!   GeistMono-Regular.ttf
//!   GeistMono-Medium.ttf
//! ```
//!
//! The fonts are distributed under the SIL Open Font License — see
//! `https://github.com/vercel/geist-font` for the source.

use std::path::{Path, PathBuf};

use khora_sdk::editor_ui::{FontHandle, FontPack, NamedFont};
use khora_sdk::{AssetIo, AssetSource, FileLoader};

/// File names looked up under the loader root. Listed in the order the shell
/// should install them — the first proportional / monospace face becomes the
/// primary one for that family.
const PROPORTIONAL_FILES: &[(&str, &str)] = &[
    ("geist-regular", "fonts/Geist-Regular.ttf"),
    ("geist-medium", "fonts/Geist-Medium.ttf"),
    ("geist-semibold", "fonts/Geist-SemiBold.ttf"),
];

const MONOSPACE_FILES: &[(&str, &str)] = &[
    ("geist-mono-regular", "fonts/GeistMono-Regular.ttf"),
    ("geist-mono-medium", "fonts/GeistMono-Medium.ttf"),
];

/// Attempts to load the Khora brand font pack via the asset I/O layer.
///
/// Tries every candidate root in order (next to the binary, then the crate's
/// `assets/` directory), and the first root containing at least one of the
/// expected files becomes the active loader. Returns an empty pack if none of
/// the roots have any of the fonts — callers should treat that as a no-op.
pub fn load_pack() -> FontPack {
    let candidates = candidate_roots();

    for root in &candidates {
        let mut loader = FileLoader::new(root);
        let proportional = collect(&mut loader, PROPORTIONAL_FILES);
        let monospace = collect(&mut loader, MONOSPACE_FILES);

        if !proportional.is_empty() || !monospace.is_empty() {
            log::info!(
                "Editor fonts: loaded {} proportional + {} monospace face(s) from '{}'.",
                proportional.len(),
                monospace.len(),
                root.display()
            );
            return FontPack {
                proportional,
                monospace,
            };
        }
    }

    log::info!(
        "Editor fonts: no Geist files found under any of {:?} \u{2014} \
         keeping default fonts. Drop the .ttf files into 'assets/fonts/' to \
         enable the brand typography.",
        candidates
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
    );
    FontPack::default()
}

/// Pulls bytes for each `(name, relative_path)` pair through `loader`,
/// silently skipping any that aren't there.
fn collect(loader: &mut dyn AssetIo, files: &[(&str, &str)]) -> Vec<NamedFont> {
    let mut out = Vec::new();
    for (name, rel) in files {
        let source = AssetSource::Path(PathBuf::from(rel));
        match loader.load_bytes(&source) {
            Ok(bytes) => out.push(NamedFont {
                name: (*name).to_owned(),
                data: FontHandle::Owned(bytes),
            }),
            Err(_) => {
                // FileLoader returns Err on missing file. We treat that as
                // "this root doesn't have this asset" — no warning, the
                // outer loop will move on.
            }
        }
    }
    out
}

/// Builds the list of root directories to try, in priority order.
fn candidate_roots() -> Vec<PathBuf> {
    let mut out = Vec::new();

    // 1. Deployed layout: `<exe-dir>/assets/`.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            out.push(parent.join("assets"));
        }
    }

    // 2. Dev layout: `<crate>/assets/` for `cargo run -p khora-editor`.
    out.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("assets"));

    out
}

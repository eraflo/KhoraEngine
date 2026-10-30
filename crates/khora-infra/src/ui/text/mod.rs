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

//! UI text backends: [`ParleyShaper`] shapes with Parley, [`SwashRasterizer`]
//! rasterises with swash. Both come with `Font::BUILT_IN` registered with
//! [`DEFAULT_FONT`].

pub mod parley_shaper;
pub mod swash_rasterizer;

pub use parley_shaper::ParleyShaper;
pub use swash_rasterizer::SwashRasterizer;

/// The engine's default UI font (Inter 4.1, variable), registered under
/// `Font::BUILT_IN` by every text backend. SIL Open Font License 1.1; the
/// licence sits beside the file in `assets/fonts/`.
pub const DEFAULT_FONT: &[u8] = include_bytes!("../../../assets/fonts/InterVariable.ttf");

/// The family a font file declares, its first face registered into
/// `collection` alone — the one rule both backends read a font file by, so
/// that they agree on what is a font (a file cut short is refused by both)
/// and on which face of a collection (`.ttc`) is drawn: its first.
pub(crate) fn register_first_face(
    collection: &mut parley::fontique::Collection,
    data: std::sync::Arc<[u8]>,
) -> Result<String, khora_core::ui::text::FontError> {
    use khora_core::ui::text::FontError;
    let registered = collection.register_fonts(
        parley::fontique::Blob::new(std::sync::Arc::new(Bytes(data))),
        None,
    );
    let (first_family, first) = registered
        .iter()
        .find_map(|(family, faces)| {
            faces
                .iter()
                .find(|face| face.index() == 0)
                .map(|face| (*family, face.clone()))
        })
        .ok_or(FontError::Unreadable)?;
    // Every other face goes: the shaper would otherwise pick, among a
    // collection's faces, the one nearest the weight it asks for, while the
    // rasteriser draws the first.
    for (family, faces) in &registered {
        for face in faces.iter().filter(|face| face.index() != 0) {
            let same_slot = *family == first_family
                && face.width() == first.width()
                && face.style() == first.style()
                && face.weight() == first.weight();
            if !same_slot {
                collection.unregister_font(*family, face.width(), face.style(), face.weight());
            }
        }
    }
    collection
        .family_name(first_family)
        .map(str::to_owned)
        .ok_or(FontError::Unreadable)
}

/// Font bytes as Parley reads them.
struct Bytes(std::sync::Arc<[u8]>);

impl AsRef<[u8]> for Bytes {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

#[cfg(test)]
mod tests;

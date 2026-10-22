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

//! The bytes a glTF file points at: its buffers and its images, embedded in
//! the file, written into a `data:` URI, or stored in files next to it.
//!
//! glTF resolves a relative URI against the glTF file's own location, so a
//! Blender export (`thing.gltf` beside `thing.bin`) works wherever it sits
//! under `assets/`. A URI is percent-decoded, and one that would leave the
//! assets root (an absolute path, a scheme other than `data:`, or `..` past
//! the root) is refused: the file is untrusted input.

use std::error::Error;
use std::path::Path;

use base64::Engine;
use khora_core::asset::asset_key;

use super::gltf_checks::{check_image, view_bytes};
use super::GltfResourceResolver;

type BoxError = Box<dyn Error + Send + Sync>;

/// Every buffer of the file, in document order, so `buffer.index()` indexes
/// the result. `base_dir` is the glTF file's folder, relative to the assets
/// root.
pub(super) fn load_buffers(
    gltf: &gltf::Gltf,
    resolver: &dyn GltfResourceResolver,
    base_dir: &Path,
) -> Result<Vec<Vec<u8>>, BoxError> {
    let mut buffers = Vec::with_capacity(gltf.buffers().len());
    for buffer in gltf.buffers() {
        let bytes = match buffer.source() {
            // glTF: only the first buffer may be the GLB's BIN chunk.
            gltf::buffer::Source::Bin if buffer.index() != 0 => {
                return Err(format!(
                    "glTF buffer {} names the BIN chunk, which only buffer 0 may",
                    buffer.index()
                )
                .into());
            }
            gltf::buffer::Source::Bin => gltf
                .blob
                .clone()
                .ok_or("GLB file references binary chunk but it is missing")?,
            gltf::buffer::Source::Uri(uri) if uri.starts_with("data:") => decode_data_uri(uri)?,
            gltf::buffer::Source::Uri(uri) => {
                resolver.resolve_buffer(&resolve_relative_uri(base_dir, uri)?)?
            }
        };
        if bytes.len() < buffer.length() {
            return Err(format!(
                "glTF buffer {} holds {} bytes, but declares {}",
                buffer.index(),
                bytes.len(),
                buffer.length()
            )
            .into());
        }
        buffers.push(bytes);
    }
    Ok(buffers)
}

/// The encoded bytes (PNG, JPEG…) of `image`, a member of `document`.
pub(super) fn image_bytes(
    document: &gltf::Document,
    image: &gltf::Image<'_>,
    buffers: &[Vec<u8>],
    resolver: &dyn GltfResourceResolver,
    base_dir: &Path,
) -> Result<Vec<u8>, BoxError> {
    // `Image::source` unwraps what this checks.
    check_image(document, image.index())?;
    match image.source() {
        gltf::image::Source::View { view, .. } => view_bytes(&view, buffers).map(<[u8]>::to_vec),
        gltf::image::Source::Uri { uri, .. } if uri.starts_with("data:") => decode_data_uri(uri),
        gltf::image::Source::Uri { uri, .. } => {
            resolver.resolve_image(&resolve_relative_uri(base_dir, uri)?)
        }
    }
}

/// The bytes of a base64 `data:` URI, whatever its media type.
pub(super) fn decode_data_uri(uri: &str) -> Result<Vec<u8>, BoxError> {
    let (header, data) = uri
        .strip_prefix("data:")
        .and_then(|rest| rest.split_once(','))
        .ok_or_else(|| format!("Unsupported data URI format: {}", truncated(uri)))?;
    if !header.ends_with(";base64") {
        return Err(format!(
            "Unsupported data URI format (not base64): {}",
            truncated(uri)
        )
        .into());
    }
    base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(Into::into)
}

/// The asset-root-relative path (forward slashes) a relative URI names from
/// `base_dir`, or why it may not be read.
pub(super) fn resolve_relative_uri(base_dir: &Path, uri: &str) -> Result<String, BoxError> {
    let decoded = percent_decode(uri)
        .ok_or_else(|| format!("glTF URI '{uri}' is not valid percent-encoded UTF-8"))?;
    let is_absolute = decoded.starts_with('/')
        || decoded.starts_with('\\')
        || decoded.contains(':')
        || Path::new(&decoded).is_absolute();
    if is_absolute {
        return Err(format!(
            "glTF URI '{uri}' is absolute or names a scheme; only paths relative to the glTF \
             file are read"
        )
        .into());
    }

    let mut parts: Vec<String> = asset_key(base_dir)
        .split('/')
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect();
    for part in decoded.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(format!("glTF URI '{uri}' leaves the assets root").into());
                }
            }
            name => parts.push(name.to_owned()),
        }
    }
    if parts.is_empty() {
        return Err(format!("glTF URI '{uri}' names no file").into());
    }
    Ok(parts.join("/"))
}

/// `text` with every `%XX` escape replaced by its byte, if the result is
/// UTF-8 and every escape is well formed.
fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3)?;
            if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The start of a long URI, for an error message.
fn truncated(uri: &str) -> &str {
    let end = uri
        .char_indices()
        .nth(64)
        .map_or(uri.len(), |(index, _)| index);
    &uri[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_relative_uri_resolves_from_the_gltf_folder() {
        let path = resolve_relative_uri(Path::new("models/car"), "wheel.bin").unwrap();
        assert_eq!(path, "models/car/wheel.bin");
    }

    #[test]
    fn dot_segments_stay_inside_the_assets_root() {
        let path = resolve_relative_uri(Path::new("models/car"), "./../shared/./a.bin").unwrap();
        assert_eq!(path, "models/shared/a.bin");
    }

    #[test]
    fn a_uri_climbing_past_the_root_is_refused() {
        assert!(resolve_relative_uri(Path::new("models"), "../../x.bin").is_err());
    }

    #[test]
    fn schemes_and_absolute_paths_are_refused() {
        for uri in [
            "/etc/passwd",
            "C:/x.bin",
            "file:///x.bin",
            "\\\\server\\x.bin",
        ] {
            assert!(resolve_relative_uri(Path::new(""), uri).is_err(), "{uri}");
        }
    }

    #[test]
    fn percent_escapes_are_decoded_and_malformed_ones_refused() {
        assert_eq!(
            resolve_relative_uri(Path::new(""), "my%20mesh.bin").unwrap(),
            "my mesh.bin"
        );
        assert!(resolve_relative_uri(Path::new(""), "bad%2").is_err());
        assert!(resolve_relative_uri(Path::new(""), "bad%zz.bin").is_err());
    }

    #[test]
    fn data_uris_of_any_media_type_decode() {
        assert_eq!(
            decode_data_uri("data:image/png;base64,AAEC").unwrap(),
            vec![0, 1, 2]
        );
        assert!(decode_data_uri("data:text/plain,hello").is_err());
    }
}

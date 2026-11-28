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

//! The `ExternalFileListerRegistration` inventory entry: what lists the files
//! a container file reads beside itself, without opening it.

use std::path::Path;

/// Inventory entry listing the external files of every file of one kind.
///
/// An external file (a glTF's `.bin`) is no asset of the index: the pack
/// builder reads this list to pack it beside its container. `type_name` is the
/// container file's kind name; a lister returns an empty list for bytes it
/// does not own (an OBJ file).
///
/// ```ignore
/// inventory::submit! {
///     ExternalFileListerRegistration {
///         type_name: "mesh",
///         list: list_gltf_external_files,
///     }
/// }
/// ```
pub struct ExternalFileListerRegistration {
    /// The kind name of the container files this lister reads.
    pub type_name: &'static str,
    /// Lists the external files of one file, given its bytes and its path
    /// under the assets root: forward-slash paths under that root.
    pub list: fn(&[u8], &Path) -> anyhow::Result<Vec<String>>,
}

inventory::collect!(ExternalFileListerRegistration);

/// Returns the external files the listers of the kind named `type_name` find in `bytes`.
///
/// Sorted and deduplicated.
///
/// # Errors
///
/// The first error a lister returns.
pub fn external_files_of(
    type_name: &str,
    bytes: &[u8],
    source_path: &Path,
) -> anyhow::Result<Vec<String>> {
    let mut files = Vec::new();
    for lister in inventory::iter::<ExternalFileListerRegistration> {
        if lister.type_name == type_name {
            files.extend((lister.list)(bytes, source_path)?);
        }
    }
    files.sort();
    files.dedup();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::external_files_of;

    /// **A glTF's external files are listed by its kind's lister**, under the
    /// assets root, sorted; data URIs and a kind with no lister list none.
    #[test]
    fn a_gltf_lists_its_external_files_under_the_assets_root() {
        let gltf = br#"{"asset":{"version":"2.0"},
            "buffers":[{"uri":"b.bin","byteLength":4},{"uri":"a.bin","byteLength":4},
                       {"uri":"data:application/octet-stream;base64,AAAAAA==","byteLength":4}],
            "images":[{"uri":"../textures/skin.png"}]}"#;
        let listed = external_files_of("mesh", gltf, Path::new("models/hero.gltf"))
            .expect("the glTF lists its files");
        assert_eq!(
            listed,
            ["models/a.bin", "models/b.bin", "textures/skin.png"]
        );
        assert!(external_files_of("texture", gltf, Path::new("a.png"))
            .expect("no lister, no error")
            .is_empty());
    }
}

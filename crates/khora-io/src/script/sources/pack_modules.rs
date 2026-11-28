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

//! The modules a game's pack holds.

use std::any::TypeId;
use std::collections::BTreeMap;

use khora_core::asset::{asset_key, kinds, AssetUUID, ScriptModule};

use super::ModuleSource;
use crate::asset::{AssetIndex, AssetReader};
use crate::script::layout::module_of;

/// The modules a game's pack holds, each read once through the pack's reader.
///
/// A module is read as the script decoder reads it: its bytes checked against
/// the signed manifest, as text, within the `Text` size limit at the reader's
/// trust. One refused is held with no source, and reported to the reader's
/// refusals.
pub(crate) struct PackModules {
    modules: BTreeMap<String, Vec<String>>,
    sources: BTreeMap<String, Option<String>>,
}

impl PackModules {
    /// Reads every module the pack behind `reader` holds.
    pub(crate) fn read(reader: &AssetReader) -> Self {
        let mut modules = BTreeMap::new();
        let mut sources = BTreeMap::new();
        for (module, uuid) in pack_modules(reader.index()) {
            let decoded = reader.decode(&uuid, TypeId::of::<ScriptModule>());
            let script = decoded.value.and_then(|value| {
                value
                    .downcast::<ScriptModule>()
                    .map_err(|_| format!("{module} does not decode to a script"))
            });
            match script {
                Ok(script) => {
                    // As the resolver reads them: `./helper.erg` is `helper.erg`.
                    let imports = (script.imports.iter())
                        .filter_map(|import| khora_script::modules::normalise(import).ok())
                        .collect();
                    modules.insert(module.clone(), imports);
                    sources.insert(module, Some(script.source));
                }
                Err(why) => {
                    log::error!("scripts: {module} is not read from the pack: {why}");
                    modules.insert(module.clone(), Vec::new());
                    sources.insert(module, None);
                }
            }
        }
        Self { modules, sources }
    }

    /// Returns no module: what a pack's compiler reads under any other root.
    pub(crate) fn none() -> Self {
        Self {
            modules: BTreeMap::new(),
            sources: BTreeMap::new(),
        }
    }
}

impl ModuleSource for PackModules {
    fn modules(&self) -> &BTreeMap<String, Vec<String>> {
        &self.modules
    }

    /// Whether the pack holds `module`, read or refused.
    fn holds(&self, module: &str) -> bool {
        self.sources.contains_key(module)
    }

    fn source(&self, module: &str) -> Option<String> {
        self.sources.get(module).cloned().flatten()
    }
}

/// Returns the modules a pack's index holds, each with the asset it is read from.
///
/// A module is a `Script` entry whose source path is a module's
/// ([`module_of`]), matched by that path: a uuid may have been frozen by the
/// identity registry under another.
pub fn pack_modules(index: &AssetIndex) -> BTreeMap<String, AssetUUID> {
    index
        .iter_all()
        .filter(|metadata| metadata.kind == kinds::SCRIPT)
        .filter_map(|metadata| {
            let rel_path = asset_key(&metadata.source_path);
            module_of(&rel_path).map(|module| (module.to_owned(), metadata.uuid))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use khora_core::asset::{AssetKind, AssetMetadata, AssetSource};

    use super::*;

    /// An index entry at `source_path`, of kind `kind`, held by `uuid`.
    fn entry(source_path: &str, kind: AssetKind, uuid: AssetUUID) -> AssetMetadata {
        AssetMetadata {
            uuid,
            source_path: PathBuf::from(source_path),
            kind,
            dependencies: Vec::new(),
            variants: HashMap::from([(
                AssetMetadata::DEFAULT_VARIANT.to_owned(),
                AssetSource::Path(PathBuf::from(source_path)),
            )]),
            tags: Vec::new(),
        }
    }

    /// **A pack's modules are its `Script` entries under `scripts/`**, each by
    /// its module name and matched by its path, whatever uuid holds it.
    #[test]
    fn a_packs_modules_are_its_script_entries_under_the_script_folder() {
        let guard = AssetUUID::new_v5("scripts/ai/guard.erg");
        // Frozen by the identity registry under the name it had before a rename.
        let main = AssetUUID::new_v5("scripts/old_main.erg");
        let entries = [
            entry("scripts/ai/guard.erg", kinds::SCRIPT, guard),
            entry("scripts/main.erg", kinds::SCRIPT, main),
            entry(
                "levels/intro.erg",
                kinds::SCRIPT,
                AssetUUID::new_v5("levels/intro.erg"),
            ),
            entry(
                "Scripts/upper.erg",
                kinds::SCRIPT,
                AssetUUID::new_v5("Scripts/upper.erg"),
            ),
            entry(
                "scripts/old.kscript",
                kinds::SCRIPT,
                AssetUUID::new_v5("scripts/old.kscript"),
            ),
            entry(
                "scripts/notes.erg",
                kinds::SHADER,
                AssetUUID::new_v5("scripts/notes.erg"),
            ),
            entry(
                "scripts/sky.png",
                kinds::TEXTURE,
                AssetUUID::new_v5("scripts/sky.png"),
            ),
        ];
        let bytes = AssetIndex::encode(&entries, &[]).expect("the index encodes");
        let index = AssetIndex::new(&bytes).expect("the index reads");

        assert_eq!(
            pack_modules(&index),
            BTreeMap::from([
                ("ai/guard.erg".to_owned(), guard),
                ("main.erg".to_owned(), main),
            ])
        );
    }
}

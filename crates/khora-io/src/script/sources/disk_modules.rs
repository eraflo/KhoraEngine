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

//! The modules under an assets root's script folder, read from disk.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use khora_script::modules::SourceLoader;

use super::ModuleSource;
use crate::script::compile::DiskLoader;
use crate::script::hot_reload::{is_on_disk, modules_under};
use crate::script::layout::{script_dir, SCRIPT_DIR};
use crate::trust::{FileGuard, Refusals, Trust};

/// The modules under an assets root's script folder, read from disk through a [`FileGuard`].
///
/// Each module is read once, however many modules import it.
pub(crate) struct DiskModules {
    root: PathBuf,
    modules: BTreeMap<String, Vec<String>>,
    loader: DiskLoader,
    read: RefCell<BTreeMap<String, Option<String>>>,
}

impl DiskModules {
    /// Lists the modules under `root`'s script folder, read at `trust`, refused to `refusals`.
    ///
    /// Without `refusals`, what is refused is held in memory only.
    pub(crate) fn open(root: &Path, refusals: Option<&Arc<Refusals>>, trust: Trust) -> Self {
        // The folder spelled exactly: a `Scripts/` holds no modules.
        let modules = script_dir(root)
            .map(|dir| modules_under(&dir, refusals, trust))
            .unwrap_or_default();
        let refusals = refusals.cloned().unwrap_or_else(Refusals::detached);
        let guard = FileGuard::new(root.join(SCRIPT_DIR), trust, refusals);
        Self {
            root: root.to_path_buf(),
            modules,
            loader: DiskLoader::guarded(guard),
            read: RefCell::new(BTreeMap::new()),
        }
    }
}

impl ModuleSource for DiskModules {
    fn modules(&self) -> &BTreeMap<String, Vec<String>> {
        &self.modules
    }

    /// Whether `module`'s file is there now, spelled exactly as the module.
    fn holds(&self, module: &str) -> bool {
        is_on_disk(&self.root, module)
    }

    fn source(&self, module: &str) -> Option<String> {
        self.read
            .borrow_mut()
            .entry(module.to_owned())
            .or_insert_with(|| self.loader.load(module))
            .clone()
    }
}

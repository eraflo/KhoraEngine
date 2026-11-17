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

//! Where a compile reads a project's modules from: its script folder on disk, or a game's pack.

use std::collections::BTreeMap;

use khora_script::modules::SourceLoader;

mod disk_modules;
mod pack_modules;

pub(crate) use disk_modules::DiskModules;
pub use pack_modules::pack_modules;
pub(crate) use pack_modules::PackModules;

/// The modules one compile reads, under one root.
pub(crate) trait ModuleSource {
    /// Returns every module there, each with the modules it imports.
    fn modules(&self) -> &BTreeMap<String, Vec<String>>;

    /// Returns whether `module` is there.
    fn holds(&self, module: &str) -> bool;

    /// Returns the source of `module`; `None` when it is not there, or was refused.
    fn source(&self, module: &str) -> Option<String>;
}

/// A [`SourceLoader`] serving the sources a [`ModuleSource`] holds.
pub(crate) struct SourcesOf<'a, S>(pub(crate) &'a S);

impl<S: ModuleSource> SourceLoader for SourcesOf<'_, S> {
    fn load(&self, path: &str) -> Option<String> {
        self.0.source(path)
    }
}

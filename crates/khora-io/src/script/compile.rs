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

//! Reading Ergon modules from disk.
//!
//! [`khora_script::compile_module`] resolves and compiles a module and
//! everything it imports through a [`SourceLoader`]; `khora-script` never opens
//! a file itself. [`DiskLoader`] is the loader that reads them from a directory,
//! and it lives here because reading is what this crate does.

use std::path::PathBuf;

use khora_script::modules::SourceLoader;

/// Loads module source from a directory on disk.
///
/// The script root, not the assets root: an `import` is written relative to
/// where scripts live, so that is what a path is joined to.
#[derive(Debug, Clone)]
pub struct DiskLoader {
    root: PathBuf,
}

impl DiskLoader {
    /// A loader rooted at `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl SourceLoader for DiskLoader {
    fn load(&self, path: &str) -> Option<String> {
        // The path arrives normalised — the resolver has already refused
        // anything absolute or containing `..`, so joining it cannot leave the
        // root. That check belongs there rather than here: it is a property of
        // the language, and a second loader would have to repeat it.
        std::fs::read_to_string(self.root.join(path)).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_disk_loader_reads_from_its_root() {
        let dir = tempfile::TempDir::new().expect("a temp dir");
        std::fs::write(dir.path().join("guard.erg"), "fn void Main() { }").expect("writes");

        let loader = DiskLoader::new(dir.path());
        assert!(loader.load("guard.erg").is_some());
        assert!(loader.load("absent.erg").is_none());
    }
}

#[cfg(test)]
mod shipped_script_tests {
    use super::*;
    use khora_script::compile_module;

    /// **Every `.erg` versioned in this repository compiles.**
    ///
    /// The sandbox's scripts are the only worked examples the documentation has,
    /// and a broken example is worse than none — a reader cannot tell their
    /// mistake from ours. This walks the tree rather than naming files, so a
    /// script added tomorrow is covered without anyone remembering to add it.
    #[test]
    fn the_sandbox_scripts_compile() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/sandbox/assets/scripts");
        let Ok(root) = root.canonicalize() else {
            // A checkout without the example tree is not a failure of this crate.
            return;
        };

        let loader = DiskLoader::new(&root);
        let mut compiled_any = false;

        for entry in std::fs::read_dir(&root).expect("script directory is readable") {
            let path = entry.expect("directory entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("erg") {
                continue;
            }
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .expect("utf-8 file name");

            let result = compile_module(&loader, name);

            assert!(
                result.diagnostics.is_empty(),
                "{name} does not compile: {:?}",
                result.diagnostics
            );
            assert!(
                result.program.is_some(),
                "{name} produced no program despite reporting no diagnostic"
            );
            compiled_any = true;
        }

        assert!(
            compiled_any,
            "no `.erg` found under {} — the worked examples are the proof the \
             chain works, and losing them would be silent",
            root.display()
        );
    }
}

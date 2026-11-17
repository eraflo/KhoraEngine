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

//! The asset loader of loose files on disk, for the editor and development builds.

use anyhow::{bail, Context, Result};
use khora_core::asset::AssetSource;
use std::path::{Path, PathBuf};

use super::{AssetIo, AssetWriter};
use crate::trust::{FileGuard, Refusals, Trust};

/// An asset loader reading loose files under a root, typically `<project>/assets/`.
pub struct FileLoader {
    guard: FileGuard,
}

impl FileLoader {
    /// Returns a loader reading the files under `root` at a trusted project's trust.
    ///
    /// Its refusals stay in memory: for a test or a tool with none of its own.
    /// A run reads through [`Self::guarded`].
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self::guarded(FileGuard::new(root, Trust::Project, Refusals::detached()))
    }

    /// Returns a loader reading every file through `guard`, under the guard's root.
    pub fn guarded(guard: FileGuard) -> Self {
        Self { guard }
    }

    /// Returns the root directory the loader reads/writes from.
    pub fn root(&self) -> &Path {
        self.guard.root()
    }
}

impl AssetIo for FileLoader {
    fn load_bytes(&self, source: &AssetSource) -> Result<Vec<u8>> {
        match source {
            AssetSource::Path(rel) => self.guard.read(rel).with_context(|| {
                format!("Failed to read asset: {:?}", self.guard.root().join(rel))
            }),
            AssetSource::Packed { .. } => {
                bail!("FileLoader does not support Packed sources")
            }
            AssetSource::Sub { container, label } => {
                bail!("FileLoader: sub-asset '{label}' of {container} has no bytes of its own")
            }
        }
    }

    fn locate(&self, source: &AssetSource) -> Option<PathBuf> {
        match source {
            AssetSource::Path(rel) => Some(self.guard.root().join(rel)),
            _ => None,
        }
    }
}

impl AssetWriter for FileLoader {
    /// Writes the file whole: a save that fails leaves the previous file as it was.
    fn write_bytes(&self, rel_path: &Path, bytes: &[u8]) -> Result<()> {
        let full_path = self.guard.root().join(rel_path);
        crate::fs::write_atomic(&full_path, bytes)
            .with_context(|| format!("Failed to write asset: {:?}", full_path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn write_bytes_creates_parent_dirs() {
        let dir = tempdir().unwrap();
        let loader = FileLoader::new(dir.path());
        loader
            .write_bytes(Path::new("scenes/default.kscene"), b"SCN")
            .unwrap();
        let written = std::fs::read(dir.path().join("scenes").join("default.kscene")).unwrap();
        assert_eq!(written, b"SCN");
    }

    #[test]
    fn a_save_over_a_file_replaces_it_whole_and_leaves_nothing_beside() {
        let dir = tempdir().unwrap();
        let loader = FileLoader::new(dir.path());
        let rel = Path::new("scenes/level.kscene");
        loader.write_bytes(rel, b"the first, longer save").unwrap();
        loader.write_bytes(rel, b"second").unwrap();
        let folder = dir.path().join("scenes");
        assert_eq!(
            std::fs::read(folder.join("level.kscene")).unwrap(),
            b"second"
        );
        let names: Vec<_> = std::fs::read_dir(&folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("level.kscene")]);
    }

    /// A save the system refuses leaves what was there before.
    #[test]
    fn a_failed_save_leaves_the_previous_file_intact() {
        let dir = tempdir().unwrap();
        let loader = FileLoader::new(dir.path());
        loader.write_bytes(Path::new("a.kscene"), b"kept").unwrap();
        // A folder where the file goes: the write lands nowhere.
        std::fs::create_dir(dir.path().join("b.kscene")).unwrap();
        std::fs::write(dir.path().join("b.kscene").join("inside"), b"kept too").unwrap();
        assert!(loader.write_bytes(Path::new("b.kscene"), b"new").is_err());
        assert_eq!(std::fs::read(dir.path().join("a.kscene")).unwrap(), b"kept");
        assert_eq!(
            std::fs::read(dir.path().join("b.kscene").join("inside")).unwrap(),
            b"kept too"
        );
        let mut names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        names.sort();
        assert_eq!(names, vec!["a.kscene", "b.kscene"], "no temporary left");
    }

    #[test]
    fn round_trip_via_load_bytes() {
        let dir = tempdir().unwrap();
        let loader = FileLoader::new(dir.path());
        loader
            .write_bytes(Path::new("textures/foo.png"), b"PNG")
            .unwrap();

        let reader = FileLoader::new(dir.path());
        let bytes = reader
            .load_bytes(&AssetSource::Path(PathBuf::from("textures/foo.png")))
            .unwrap();
        assert_eq!(bytes, b"PNG");
    }

    /// A sub-asset has no bytes of its own; the service reads its container.
    #[test]
    fn load_bytes_refuses_a_sub_source() {
        let dir = tempdir().unwrap();
        let reader = FileLoader::new(dir.path());
        let result = reader.load_bytes(&AssetSource::Sub {
            container: khora_core::asset::AssetUUID::new_v5("models/hero.glb"),
            label: "mesh/Body".to_string(),
        });
        assert!(result.is_err(), "a Sub source has no file to read");
    }

    /// One loader is shared by every thread decoding assets: each read
    /// returns its own file's bytes, whatever the other threads read.
    #[test]
    fn a_file_loader_read_from_many_threads_at_once_returns_each_file_exactly() {
        const FILES: usize = 16;
        const THREADS: usize = 8;
        const READS: usize = 200;

        let dir = tempdir().unwrap();
        let writer = FileLoader::new(dir.path());
        let contents: Vec<Vec<u8>> = (0..FILES)
            .map(|i| {
                (0..(64 + i * 37))
                    .map(|j| ((i * 31 + j) % 251) as u8)
                    .collect()
            })
            .collect();
        for (i, bytes) in contents.iter().enumerate() {
            writer
                .write_bytes(Path::new(&format!("blobs/{i}.bin")), bytes)
                .unwrap();
        }

        let loader = FileLoader::new(dir.path());
        std::thread::scope(|scope| {
            for thread in 0..THREADS {
                let (loader, contents) = (&loader, &contents);
                scope.spawn(move || {
                    for read in 0..READS {
                        let i = (thread * 7 + read) % FILES;
                        let source = AssetSource::Path(PathBuf::from(format!("blobs/{i}.bin")));
                        let bytes = loader
                            .load_bytes(&source)
                            .unwrap_or_else(|e| panic!("file {i} reads: {e:#}"));
                        assert_eq!(bytes, contents[i], "file {i}, thread {thread}, read {read}");
                    }
                });
            }
        });
    }
}

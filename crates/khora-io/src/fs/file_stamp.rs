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

//! What tells one write of a file from another: its length and its modified time.

use std::path::Path;
use std::time::SystemTime;

/// A file's length and modified time: two reads seeing the same stamp saw the same write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileStamp {
    /// Its length, in bytes.
    pub len: u64,
    /// When it was last modified.
    pub modified: SystemTime,
}

impl FileStamp {
    /// Returns the stamp of the file at `path`; `None` when it has no metadata or time to read.
    pub fn of(path: &Path) -> Option<FileStamp> {
        Self::of_metadata(&std::fs::metadata(path).ok()?)
    }

    /// Returns the stamp `metadata` gives; `None` when the system keeps no modified time.
    pub fn of_metadata(metadata: &std::fs::Metadata) -> Option<FileStamp> {
        Some(FileStamp {
            len: metadata.len(),
            modified: metadata.modified().ok()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_has_its_length_as_stamp_and_a_missing_one_has_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        assert_eq!(FileStamp::of(&path), None);
        std::fs::write(&path, b"12345").unwrap();
        let stamp = FileStamp::of(&path).expect("a file has a stamp");
        assert_eq!(stamp.len, 5);
        let metadata = std::fs::metadata(&path).unwrap();
        assert_eq!(FileStamp::of_metadata(&metadata), Some(stamp));
    }

    #[test]
    fn a_rewrite_of_another_length_changes_the_stamp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, b"one").unwrap();
        let before = FileStamp::of(&path);
        std::fs::write(&path, b"three").unwrap();
        assert_ne!(FileStamp::of(&path), before);
    }
}

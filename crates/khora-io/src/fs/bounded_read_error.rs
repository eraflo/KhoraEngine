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

//! Why a bounded read returned no bytes.

use super::FileStamp;

/// Why [`read_bounded`](super::read_bounded) returned no bytes.
#[derive(Debug, thiserror::Error)]
pub enum BoundedReadError {
    /// The file is larger than the cap.
    #[error("{size} bytes, over the {cap}-byte cap")]
    TooLarge {
        /// Its size, in bytes: as its metadata gave it, or as much as was read.
        size: u64,
        /// The cap, in bytes.
        cap: u64,
        /// Its stamp when the size came from its metadata; `None` when it grew during the read.
        stamp: Option<FileStamp>,
    },
    /// The file could not be opened or read.
    #[error("{0}")]
    Io(#[source] std::io::Error),
}

impl BoundedReadError {
    /// Returns whether the file was missing.
    pub fn is_not_found(&self) -> bool {
        matches!(self, BoundedReadError::Io(error) if error.kind() == std::io::ErrorKind::NotFound)
    }
}

impl From<BoundedReadError> for std::io::Error {
    fn from(error: BoundedReadError) -> Self {
        match error {
            BoundedReadError::Io(error) => error,
            too_large => {
                std::io::Error::new(std::io::ErrorKind::InvalidData, too_large.to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_is_not_found_and_a_large_one_is_not() {
        let missing = BoundedReadError::Io(std::io::ErrorKind::NotFound.into());
        let large = BoundedReadError::TooLarge {
            size: 10,
            cap: 4,
            stamp: None,
        };
        assert!(missing.is_not_found());
        assert!(!large.is_not_found());
    }

    #[test]
    fn a_large_file_becomes_invalid_data_naming_its_size_and_cap() {
        let error: std::io::Error = BoundedReadError::TooLarge {
            size: 10,
            cap: 4,
            stamp: None,
        }
        .into();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(error.to_string(), "10 bytes, over the 4-byte cap");
    }
}

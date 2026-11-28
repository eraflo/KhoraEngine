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

//! Reading a file under a cap on its size.

use std::io::Read;
use std::path::Path;

use super::{BoundedReadError, FileStamp};

/// Returns the bytes of the file at `path`, refused past `cap` bytes.
///
/// Its size is read from the open file and the read stops at `cap + 1` bytes,
/// so a file growing during the read is refused too.
///
/// # Errors
///
/// [`BoundedReadError::TooLarge`] past `cap`; [`BoundedReadError::Io`] when the
/// file cannot be opened or read (`NotFound` for a missing one).
pub fn read_bounded(path: &Path, cap: u64) -> Result<Vec<u8>, BoundedReadError> {
    let file = std::fs::File::open(path).map_err(BoundedReadError::Io)?;
    let metadata = file.metadata().map_err(BoundedReadError::Io)?;
    if metadata.len() > cap {
        return Err(BoundedReadError::TooLarge {
            size: metadata.len(),
            cap,
            stamp: FileStamp::of_metadata(&metadata),
        });
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(cap.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(BoundedReadError::Io)?;
    if bytes.len() as u64 > cap {
        // It grew past its cap since its size was read.
        return Err(BoundedReadError::TooLarge {
            size: bytes.len() as u64,
            cap,
            stamp: None,
        });
    }
    Ok(bytes)
}

/// Returns the text of the file at `path`, refused past `cap` bytes, as [`read_bounded`].
///
/// # Errors
///
/// As [`read_bounded`], and [`BoundedReadError::Io`] of kind `InvalidData`
/// when the file is not UTF-8.
pub fn read_bounded_to_string(path: &Path, cap: u64) -> Result<String, BoundedReadError> {
    let bytes = read_bounded(path, cap)?;
    String::from_utf8(bytes).map_err(|error| {
        BoundedReadError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, error))
    })
}

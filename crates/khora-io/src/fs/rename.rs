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

//! Renaming an entry, also in case only.

use std::path::Path;

use super::is_same_entry;
use super::temporary::aside_beside;

/// Renames the file or folder at `from` to `to`, also when the two name one entry in another case only.
///
/// A rename in case only goes through a temporary name beside `from`, as not
/// every system renames an entry onto itself; when its second step fails, the
/// entry is put back at `from`, and one a crash left aside is put back by
/// [`reclaim_asides`](super::reclaim_asides). `to` naming `from` exactly changes nothing. Any other
/// rename is [`std::fs::rename`]'s.
///
/// # Errors
///
/// When a step of the rename fails.
pub fn rename_entry(from: &Path, to: &Path) -> std::io::Result<()> {
    if !is_same_entry(from, to) {
        return std::fs::rename(from, to);
    }
    if from.file_name() == to.file_name() {
        return Ok(());
    }
    let aside = aside_beside(from);
    std::fs::rename(from, &aside)?;
    std::fs::rename(&aside, to).inspect_err(|_| {
        let _ = std::fs::rename(&aside, from);
    })
}

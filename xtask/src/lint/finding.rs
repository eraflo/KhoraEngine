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

//! One check's finding in one file, as `cargo xtask lint-file` prints it.

use std::fmt;

/// A check's finding at one line of a file.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    /// Repository-relative path of the file, with `/` separators.
    pub path: String,
    /// 1-based line the finding points at.
    pub line: usize,
    /// The check's name, as the allowlist and the layout tests name it.
    pub check: &'static str,
    /// What is wrong there.
    pub message: String,
    /// The key the allowlist lists: the path, or `<path>::<function>`.
    pub key: String,
}

impl fmt::Display for Finding {
    /// Writes `path:line: check: message`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}: {}: {}",
            self.path, self.line, self.check, self.message
        )
    }
}

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

//! The file-system helpers every tool shares: resolving paths, writing a file
//! whole, reading one under a cap, walking a tree.

mod bounded_read;
mod bounded_read_error;
mod file_stamp;
mod keyed;
mod path_lock;
mod paths;
mod reclaimed;
mod rename;
mod replace_dir;
mod ron_store;
mod root_name;
mod temporary;
mod walk;
mod write;

pub use bounded_read::{read_bounded, read_bounded_to_string};
pub use bounded_read_error::BoundedReadError;
pub use file_stamp::FileStamp;
pub use keyed::Keyed;
pub use path_lock::PathLock;
pub(crate) use paths::resolved_within;
pub use paths::{
    is_same_entry, is_under, real_path, relative_to, resolved_relative_to, spelled_on_disk,
};
pub use reclaimed::Reclaimed;
pub use rename::rename_entry;
pub use replace_dir::{replace_dir_with, replace_dir_with_reporting};
pub use ron_store::RonStore;
pub use root_name::RootName;
pub use temporary::{is_temporary, reclaim_asides};
pub use walk::{files_under, is_scratch};
pub use write::{write_atomic, write_atomic_with, write_new, write_new_private};

use paths::relative_to_resolved;
use write::make_parent;

#[cfg(test)]
mod tests;

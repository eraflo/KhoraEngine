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

//! Scenes written down and brought back.
//!
//! A world is captured as a [`SceneRecord`] — its entities by persistent
//! identity, their components by name — and a record is applied back
//! atomically. How a record becomes bytes is an [`encoding`]; which one is
//! a matter of how the file will be used, never of what it holds.

pub mod component_registration;
pub mod material_registration;
pub mod shape;

pub mod apply;
pub mod capture;
pub mod encoding;
pub mod file;
pub mod prefab;
pub mod record;
pub mod retired;
pub mod scene_record;

pub use component_registration::*;
pub use material_registration::*;
pub use shape::{ComponentShape, FieldSchema};

pub use apply::{apply, prepare, Applied, Identity, LoadFailure, Prepared};
pub use capture::{capture_subtree, capture_world, SaveError};
pub use encoding::{
    encoding_named, CompactEncoding, EncodingError, MsgPackEncoding, SceneEncoding, TextEncoding,
};
pub use file::{read_scene_file, write_scene_file, SceneFileReadError, UPGRADE_SCENES_COMMAND};
pub use prefab::{instantiate_subtree, serialize_subtree};
pub use retired::{is_retired, RetiredComponent};
pub use scene_record::{PageRecord, SceneRecord};

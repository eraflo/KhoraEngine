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

//! Scenes as records: a world captured into a [`SceneRecord`], and applied back atomically.
//!
//! An [`encoding`] changes how a record is written, never what it holds.
//! See the book: concepts/serialization.md, "The whole path at a glance".

pub mod component_registration;
pub mod material_registration;
pub mod shape;

pub mod apply;
pub(crate) mod asset_kinds;
pub mod capture;
mod commit;
pub mod declared;
pub mod encoding;
mod entity_refs;
pub mod file;
pub mod inspector_json;
mod kept;
pub mod load_catalog;
pub mod loading;
mod page_plan;
pub mod positional;
pub mod prefab;
pub mod record;
mod resolver;
pub mod retired;
pub mod save;
pub mod scene_record;
pub mod schema;
mod session_only_read;
pub mod snapshot;
mod staged_value;

pub use component_registration::*;
pub use material_registration::*;
pub use shape::{ComponentShape, FieldSchema};

pub use apply::{apply, prepare, prepare_with, Applied, Identity, LoadFailure, Prepared};
pub use asset_kinds::read_into;
pub use capture::{
    capture_subtree, capture_subtree_reporting, capture_world, capture_world_reporting, SaveError,
};
pub use declared::{DeclaredFieldRecord, DeclaredRecord};
pub use encoding::{
    encoding_named, CompactEncoding, EncodingError, MsgPackEncoding, SceneEncoding, TextEncoding,
};
pub use file::{encoding_of, read_scene_file, write_scene_file, SceneFileReadError};
pub use load_catalog::{CatalogDeclared, LoadCatalog};
pub use prefab::{
    apply_to_prefab, apply_to_prefab_reporting, collapse, copy_subtree, expand, expand_reporting,
    instance_of, instantiate_prefab, instantiate_prefab_at, instantiate_subtree, prefab_world,
    serialize_prefab, serialize_prefab_reporting, serialize_subtree, serialize_subtree_reporting,
    InstanceOf, NoPrefabs, PrefabApply, PrefabCatalog, PrefabLibrary, PrefabSource,
};
pub use retired::{is_retired, RetiredComponent};
pub use save::{
    about_what_loads, capture_save, capture_save_reporting, compose, compose_reporting,
    prepare_game, prepare_game_with, promote, read_save_file, write_save_file, RemovedComponents,
    SaveRecord,
};
pub use scene_record::{InstanceRecord, PageRecord, SceneRecord};

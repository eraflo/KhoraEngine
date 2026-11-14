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

//! Asset I/O and decoding services.

mod asset_service;
mod asset_writer;
mod change;
mod container;
mod decoder;
mod decoder_registration;
mod decoder_registry;
pub mod decoders;
mod defaults;
pub(crate) mod dependencies;
mod file;
mod generations;
mod id_registry;
mod index;
mod index_hub;
pub use index_hub::{install_indexing, IndexHub, IndexOutcome};
mod index_builder;
mod io;
mod manifest;
mod off_frame;
pub use off_frame::{attach_decoding, install_decoding, DecodeHub};
mod pack;
mod pack_builder;
pub mod pack_key;
mod reader;
mod requested;
pub mod root_path;
pub mod signing;
mod sub_asset_lister_registration;
mod watch;
mod watcher;

pub use asset_service::*;
pub use change::*;
pub use container::*;
pub use decoder::*;
pub use decoder_registry::*;
pub use decoders::*;
pub use dependencies::*;
pub use file::*;
pub use id_registry::*;
pub use index::{AssetIndex, MAX_INDEX_BYTES};
pub use index_builder::*;
pub use io::*;
pub use manifest::*;
pub use pack::*;
pub use pack_builder::*;
pub use reader::*;
pub use requested::Requested;
pub use watch::*;
pub use watcher::*;

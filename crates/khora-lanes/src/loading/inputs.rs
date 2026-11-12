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

//! What a load reads: the requests waiting, who loads each kind, its budget.

use std::sync::Arc;

use khora_core::asset::{AssetDecoding, AssetIndexing, DecodeRequest, IndexRequest};
use khora_core::lane::io::Budget;
use khora_data::scene::loading::{SceneLoadRequest, SceneLoading};
use khora_macros::LaneInputs;
use khora_script::toolchain::{CompileRequest, ScriptCompiler};

/// What a load reads: the requests of each kind waiting, taken; who prepares,
/// compiles and decodes them; its budget. Nothing of a frame — a run may span
/// several. What a run leaves in a `#[take]` field goes back to its channel.
#[derive(LaneInputs)]
pub struct LoadingInputs<'f> {
    /// The project indexes filed since the last run.
    #[take]
    pub indexes: Vec<IndexRequest>,
    /// The scene loads filed since the last run.
    #[take]
    pub scenes: Vec<SceneLoadRequest>,
    /// The script compiles filed since the last run.
    #[take]
    pub scripts: Vec<CompileRequest>,
    /// The asset decodes filed since the last run.
    #[take]
    pub assets: Vec<DecodeRequest>,
    /// Who indexes the projects.
    #[resource]
    pub indexing: &'f Arc<dyn AssetIndexing>,
    /// Who prepares the scene loads.
    #[resource]
    pub loading: &'f Arc<dyn SceneLoading>,
    /// Who compiles the scripts.
    #[resource]
    pub compiler: &'f Arc<dyn ScriptCompiler>,
    /// Who decodes the assets.
    #[resource]
    pub decoding: &'f Arc<dyn AssetDecoding>,
    /// The time the run is lent.
    #[budget]
    pub budget: Option<Budget>,
}

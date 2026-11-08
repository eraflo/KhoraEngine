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

//! Scene loads prepared off the frame.
//!
//! A host files a [`SceneLoadRequest`]; the `loading` family's lane takes
//! it and prepares it off the frame through the runtime's [`SceneLoading`],
//! staging the [`PreparedLoad`] under a lease on the world's components
//! staying as they were when it was prepared. A frame's boundary commits it.
//! A load whose lease broke is abandoned there, and filed again.

use std::sync::Arc;

use khora_core::ecs::ReservationHold;
use khora_core::event::{Channel, Supersedes};
use khora_core::lane::{Lease, Staged};

pub use khora_core::scene::LoadTicket;

use super::apply::Prepared;
use super::prefab::PrefabSource;
use super::record::ReportEntry;

/// Where a scene load's bytes come from, fetched off the frame.
pub trait SceneFetch: Send + Sync {
    /// The bytes of the scene file, read off the frame; why not, when they
    /// cannot be read.
    fn fetch(&self) -> Result<Vec<u8>, String>;
}

/// The bytes a scene load reads.
#[derive(Clone)]
pub enum SceneSource {
    /// The scene file's bytes, at hand.
    Bytes(Arc<[u8]>),
    /// The scene file's bytes, fetched off the frame.
    Fetch(Arc<dyn SceneFetch>),
}

impl std::fmt::Debug for SceneSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bytes(bytes) => write!(f, "Bytes({} bytes)", bytes.len()),
            Self::Fetch(_) => f.write_str("Fetch"),
        }
    }
}

/// What a scene load does to the world.
#[derive(Debug, Clone)]
pub enum LoadKind {
    /// Brings the scene in beside what the world holds.
    Add(SceneSource),
    /// Replaces everything the world holds with the scene.
    Replace(SceneSource),
    /// Replaces everything the world holds with the game `save` holds,
    /// taken against the scene `base` holds.
    Game {
        /// The game save file.
        save: SceneSource,
        /// The scene file the save was taken against.
        base: SceneSource,
    },
}

/// A scene load filed to be prepared off the frame.
pub struct SceneLoadRequest {
    /// What its outcome is filed under.
    pub ticket: LoadTicket,
    /// What the load does, and what it reads.
    pub kind: LoadKind,
    /// Where the prefabs the scene links to are read from.
    pub prefabs: Arc<dyn PrefabSource + Send + Sync>,
    /// The ids the load reserves are held by, when the host gave one; a new
    /// hold from the world's reserver otherwise.
    pub hold: Option<ReservationHold>,
}

impl Clone for SceneLoadRequest {
    /// The same load, without the hold — which holds ids for one load only:
    /// the copy reserves through a new hold when it is prepared.
    fn clone(&self) -> Self {
        Self {
            ticket: self.ticket,
            kind: self.kind.clone(),
            prefabs: Arc::clone(&self.prefabs),
            hold: None,
        }
    }
}

impl std::fmt::Debug for SceneLoadRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SceneLoadRequest")
            .field("ticket", &self.ticket)
            .field("kind", &self.kind)
            .field("hold", &self.hold.is_some())
            .finish()
    }
}

impl Supersedes for SceneLoadRequest {}

/// What a load abandoned at a frame's boundary files again: its request, on
/// the channel it was taken from, ahead of those filed since.
pub struct Refile {
    /// The channel the request was taken from.
    pub requests: Channel<SceneLoadRequest>,
    /// The request, filed again with the hold of the load abandoned.
    pub request: SceneLoadRequest,
}

/// A scene load prepared off the frame, staged for a frame's boundary to
/// commit.
pub struct PreparedLoad {
    /// The ticket of the request it answers.
    pub ticket: LoadTicket,
    /// Whether it replaces everything the world holds.
    pub replaces: bool,
    /// The load prepared, and what reading its file adapted before the
    /// prepare (prefab expansion, merges); why not, when it failed.
    pub outcome: Result<(Prepared, Vec<ReportEntry>), String>,
    /// What to file again should it be abandoned.
    pub refile: Option<Refile>,
}

impl std::fmt::Debug for PreparedLoad {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedLoad")
            .field("ticket", &self.ticket)
            .field("replaces", &self.replaces)
            .field("prepared", &self.outcome.is_ok())
            .finish()
    }
}

impl Supersedes for PreparedLoad {}

impl Staged for PreparedLoad {
    /// Files its request again, ahead of any request filed since and with
    /// the hold its ids were reserved through: the load is prepared afresh
    /// against the world as it is now, reusing those ids.
    fn abandon(self) {
        let Self {
            outcome, refile, ..
        } = self;
        let hold = outcome.ok().map(|(prepared, _)| prepared.into_hold());
        if let Some(Refile {
            requests,
            mut request,
        }) = refile
        {
            request.hold = hold;
            // Given back as a taken request is, past the channel's bound: a
            // load filed is never lost for being prepared again.
            requests.restore(vec![request]);
        }
    }
}

/// What prepares a scene load off the frame: the runtime's, read by the
/// `loading` family's lanes.
pub trait SceneLoading: Send + Sync {
    /// Prepares `request`, and the lease the result holds to: it breaks when
    /// the world's components change before the load commits.
    fn prepare_leased(&self, request: SceneLoadRequest) -> (PreparedLoad, Option<Lease>);
}

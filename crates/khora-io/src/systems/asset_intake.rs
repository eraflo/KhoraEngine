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

//! The assets decoded off the frame, taken in at its boundary into the
//! [`AssetService`]'s cache and the engine's [`AssetStore`], before anything
//! of the frame reads an asset.

use std::sync::{Arc, Mutex};

use khora_core::asset::Decoded;
use khora_core::lane::{OutputDeck, Reserve};
use khora_core::Runtime;
use khora_data::assets::AssetStore;
use khora_data::ecs::{DataSystemRegistration, SystemContext, TickPhase, World};

use crate::asset::AssetService;

/// Takes in every decode the reserve holds into the runtime's asset service and store.
///
/// Nothing without a reserve, a service or a store.
pub fn take_in_decoded(runtime: &Runtime) {
    // A service handed over since the last boundary is served from now on.
    crate::asset::attach_decoding(runtime);
    let Some(reserve) = runtime.look_up::<Reserve>() else {
        return;
    };
    let decoded = reserve.take::<Decoded>();
    if decoded.is_empty() {
        return;
    }
    let (Some(service), Some(store)) = (
        runtime.services.get::<Arc<Mutex<AssetService>>>(),
        runtime.resources.get::<AssetStore>(),
    ) else {
        log::warn!(
            "{} decoded asset(s) landed, but the engine holds no asset service or store",
            decoded.len()
        );
        return;
    };
    let mut service = service
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for decoded in decoded {
        service.take_in(decoded, store);
    }
}

fn asset_intake_system(
    _world: &mut World,
    runtime: &Runtime,
    _deck: &mut OutputDeck,
    _ctx: SystemContext,
) {
    take_in_decoded(runtime);
}

// At the frame boundary, whatever families are on: an editor editing a
// scene shows the meshes and textures it loads.
inventory::submit! {
    DataSystemRegistration {
        name: "asset_intake",
        phase: TickPhase::Boundary,
        run: asset_intake_system,
        order_hint: 0,
        runs_after: &[],
        observes: &[],
        role: khora_data::ecs::Role::Upkeep,
    }
}

/// The `loading` family's work, played by hand for the tests of the systems that
/// ask for assets.
#[cfg(test)]
pub(crate) mod tests {
    use std::sync::{Arc, Mutex};

    use khora_core::asset::AssetDecoding;
    use khora_core::lane::{Reserve, Staging};
    use khora_core::Runtime;

    use crate::asset::AssetService;

    /// Plays one run of the `loading` family, and returns how many requests it took.
    ///
    /// The requests the runtime's asset service filed are decoded, staged under
    /// their leases and received by the runtime's `Reserve`, as a frame start does.
    pub(crate) fn decode_filed(runtime: &Runtime) -> usize {
        decode_filed_while(runtime, || {})
    }

    /// Plays [`decode_filed`], running `during` between the decodes and their reception.
    ///
    /// `during` is what the host does while the run is out (a file saved again,
    /// an asset invalidated), checked by each decode's lease.
    pub(crate) fn decode_filed_while(runtime: &Runtime, during: impl FnOnce()) -> usize {
        let service = runtime
            .services
            .get::<Arc<Mutex<AssetService>>>()
            .expect("the runtime holds an asset service");
        let reserve = runtime
            .look_up::<Reserve>()
            .expect("the runtime holds the reserve the `loading` family stages into");
        let filed = service
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .decode_requests()
            .drain();
        let decoding: Arc<dyn AssetDecoding> = AssetService::decoding(service);
        let decoded: Vec<_> = filed
            .iter()
            .map(|request| decoding.decode_leased(request))
            .collect();
        during();
        let mut staging = Staging::new();
        for (decoded, lease) in decoded {
            match lease {
                Some(lease) => staging.stage_while(vec![decoded], lease),
                None => staging.stage(vec![decoded]),
            }
        }
        reserve.receive(staging);
        filed.len()
    }

    /// How many frames [`settle`] runs before it calls the systems stuck.
    const SETTLE_FRAMES: usize = 8;

    /// Runs frames of the systems `frame` runs until what they asked for is taken in.
    ///
    /// Done on the first frame that files nothing.
    ///
    /// # Panics
    ///
    /// When the systems still file a decode after [`SETTLE_FRAMES`] frames.
    pub(crate) fn settle(
        runtime: &Runtime,
        world: &mut khora_data::ecs::World,
        mut frame: impl FnMut(&mut khora_data::ecs::World),
    ) {
        for _ in 0..SETTLE_FRAMES {
            super::take_in_decoded(runtime);
            frame(world);
            if decode_filed(runtime) == 0 {
                return;
            }
        }
        panic!("the asset systems still file decodes after {SETTLE_FRAMES} frames");
    }
}

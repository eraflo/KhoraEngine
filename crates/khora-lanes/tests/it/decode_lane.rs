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

//! **The decode lane hands the requests it takes to the runtime's decoder,
//! and stages each result under the lease it holds to.** Run by hand as the
//! scheduler runs a background lane: its inputs fetched from a runtime
//! holding the request channel and a fake [`AssetDecoding`], its outputs
//! committed into a staging the reserve then receives. It reads no `World`,
//! no bus, and locks nothing.

use std::any::TypeId;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use khora_core::asset::{AssetDecoding, AssetUUID, DecodeRequest, Decoded};
use khora_core::event::{Channel, WhenFull};
use khora_core::lane::contract::Lane;
use khora_core::lane::io::{FrameId, InputSource, LaneInputs, LaneOutputs, OutputSink};
use khora_core::lane::{LaneBus, Lease, OutputDeck, Reserve, Staging};
use khora_core::Runtime;
use khora_lanes::decode_lane::{DecodeInputs, DecodeLane, DecodeOutputs};

/// A decoder that knows a few assets, each decoding to a number, and fails
/// any other. Each result is leased on a flag of its asset the test may
/// lower, standing for the asset's generation moving on. It remembers the
/// requests, in the order it was asked.
#[derive(Default)]
struct FakeDecoding {
    known: HashMap<AssetUUID, u32>,
    current: HashMap<AssetUUID, Arc<AtomicBool>>,
    asked: Mutex<Vec<DecodeRequest>>,
}

impl FakeDecoding {
    fn knowing(assets: &[(AssetUUID, u32)]) -> Arc<Self> {
        Arc::new(Self {
            known: assets.iter().copied().collect(),
            current: assets
                .iter()
                .map(|(uuid, _)| (*uuid, Arc::new(AtomicBool::new(true))))
                .collect(),
            asked: Mutex::new(Vec::new()),
        })
    }

    fn asked(&self) -> Vec<DecodeRequest> {
        self.asked
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// The asset `uuid` changed since it was asked for: what was decoded for
    /// it no longer counts.
    fn change(&self, uuid: AssetUUID) {
        self.current[&uuid].store(false, Ordering::SeqCst);
    }
}

impl AssetDecoding for FakeDecoding {
    fn decode(&self, uuid: &AssetUUID, type_id: TypeId) -> Decoded {
        Decoded {
            uuid: *uuid,
            type_id,
            value: self
                .known
                .get(uuid)
                .map(|value| Box::new(*value) as Box<dyn std::any::Any + Send>)
                .ok_or_else(|| format!("asset {uuid} is not known")),
            external_files: Vec::new(),
        }
    }

    fn decode_leased(&self, request: &DecodeRequest) -> (Decoded, Option<Lease>) {
        self.asked
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(request.clone());
        let lease = self.current.get(&request.uuid).map(|current| {
            let current = Arc::clone(current);
            Lease::new(move || current.load(Ordering::SeqCst))
        });
        (self.decode(&request.uuid, request.type_id), lease)
    }
}

fn request(uuid: AssetUUID) -> DecodeRequest {
    DecodeRequest {
        uuid,
        type_id: TypeId::of::<u32>(),
        generation: 0,
    }
}

/// A runtime holding the request channel, filled with `requests`, and
/// `decoding` as the engine installs it.
fn runtime_with(requests: &[DecodeRequest], decoding: Arc<FakeDecoding>) -> Runtime {
    let channel = Channel::<DecodeRequest>::bounded(64, WhenFull::Reject);
    for request in requests {
        channel.send(request.clone());
    }
    let mut runtime = Runtime::new();
    runtime.resources.insert(channel);
    runtime.resources.insert(decoding as Arc<dyn AssetDecoding>);
    runtime
}

/// One run of the lane over `runtime`, as the scheduler runs it at the
/// background: its inputs fetched, the lane recorded, its outputs committed
/// into the staging returned — not yet received by a reserve.
fn run(runtime: &Runtime) -> Staging {
    let bus = LaneBus::new();
    let mut source = InputSource::new(FrameId(0), &bus, runtime);
    let mut inputs = DecodeInputs::fetch(&mut source).expect("the decode lane's inputs");
    let mut deck = OutputDeck::new();
    let mut staging = Staging::new();
    {
        let mut sink = OutputSink::new(&mut deck).with_staging(&mut staging);
        let mut outputs = DecodeOutputs::open(&mut sink).expect("the decode lane's outputs");
        DecodeLane
            .record(0, &mut inputs, &mut outputs)
            .expect("the decode runs");
        outputs.commit(&mut sink).expect("the outputs are staged");
    }
    staging
}

/// What `staging` lands in a fresh reserve, as the frame start receives it.
fn landed(staging: Staging) -> Vec<Decoded> {
    let reserve = Reserve::new();
    reserve.receive(staging);
    reserve.take::<Decoded>()
}

fn value_of(decoded: &Decoded) -> Option<u32> {
    decoded
        .value
        .as_ref()
        .ok()
        .and_then(|value| value.downcast_ref::<u32>())
        .copied()
}

/// **A run decodes every request it takes and stages one result each.** Two
/// requests wait: the decoder is asked for both, in the order they were
/// filed, each result reaches the reserve with its value, and the request
/// channel is left empty.
#[test]
fn a_run_decodes_every_request_it_takes_and_stages_one_result_each() {
    let (mesh, texture) = (
        AssetUUID::new_v5("meshes/a.obj"),
        AssetUUID::new_v5("t.png"),
    );
    let decoding = FakeDecoding::knowing(&[(mesh, 7), (texture, 9)]);
    let runtime = runtime_with(&[request(mesh), request(texture)], Arc::clone(&decoding));

    let decoded = landed(run(&runtime));

    assert_eq!(decoding.asked(), vec![request(mesh), request(texture)]);
    let got: Vec<(AssetUUID, Option<u32>)> = decoded
        .iter()
        .map(|decoded| (decoded.uuid, value_of(decoded)))
        .collect();
    assert_eq!(got, vec![(mesh, Some(7)), (texture, Some(9))]);
    assert!(
        decoded.iter().all(|d| d.type_id == TypeId::of::<u32>()),
        "each result carries the type it was asked for"
    );
    assert!(
        runtime
            .resources
            .get::<Channel<DecodeRequest>>()
            .is_some_and(|channel| channel.is_empty()),
        "the requests were taken"
    );
}

/// **A failed decode is staged, never dropped nor panicked on.** A request
/// names an asset the decoder does not know: its failure reaches the
/// reserve, so the intake can record it — beside the request that decoded.
#[test]
fn a_failed_decode_is_staged_beside_the_ones_that_decoded() {
    let known = AssetUUID::new_v5("meshes/a.obj");
    let unknown = AssetUUID::new_v5("meshes/never_indexed.obj");
    let decoding = FakeDecoding::knowing(&[(known, 1)]);
    let runtime = runtime_with(&[request(unknown), request(known)], Arc::clone(&decoding));

    let decoded = landed(run(&runtime));

    assert_eq!(decoded.len(), 2, "one result per request: {decoded:?}");
    let failed = decoded
        .iter()
        .find(|decoded| decoded.uuid == unknown)
        .expect("the failure is staged");
    assert!(failed.value.is_err(), "it is staged as a failure");
    assert!(decoded
        .iter()
        .any(|decoded| decoded.uuid == known && value_of(decoded) == Some(1)));
}

/// **Each result is staged under its own lease.** Two assets are decoded
/// in one run; one changes before the frame start receives the run: its
/// result is abandoned there, and only its — the other lands.
#[test]
fn a_result_whose_asset_changed_while_it_waited_is_abandoned_and_only_it() {
    let (changed, kept) = (AssetUUID::new_v5("a.obj"), AssetUUID::new_v5("b.obj"));
    let decoding = FakeDecoding::knowing(&[(changed, 1), (kept, 2)]);
    let runtime = runtime_with(&[request(changed), request(kept)], Arc::clone(&decoding));

    let staging = run(&runtime);
    decoding.change(changed);
    let decoded = landed(staging);

    let uuids: Vec<AssetUUID> = decoded.iter().map(|decoded| decoded.uuid).collect();
    assert_eq!(
        uuids,
        vec![kept],
        "the changed asset's decode is never installed"
    );
}

/// **A run with no request decodes nothing and stages nothing.**
#[test]
fn a_run_without_a_request_decodes_nothing() {
    let decoding = FakeDecoding::knowing(&[(AssetUUID::new_v5("a.obj"), 1)]);
    let runtime = runtime_with(&[], Arc::clone(&decoding));

    let decoded = landed(run(&runtime));

    assert!(decoding.asked().is_empty(), "the decoder was never asked");
    assert!(decoded.is_empty());
}

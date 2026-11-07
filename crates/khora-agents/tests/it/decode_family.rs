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

//! The decode family, registered and booted as the engine boots it: a
//! family of its own at `ExecutionPhase::BACKGROUND`, whose lane takes the
//! requests the asset service files and stages what the runtime's
//! `AssetDecoding` returns — off the frame, and whether or not the
//! scripting family is on.

use std::any::{Any, TypeId};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use khora_agents::agent::lane_agent;
use khora_agents::registered_agent::{lane_agents, registered_agents};
use khora_control::{AgentRegistry, Context, ExecutionScheduler};
use khora_core::agent::gorna::AgentId;
use khora_core::agent::registration::{check_registrations, AgentRegistration};
use khora_core::agent::{Activation, Agent, AgentImportance, ExecutionPhase};
use khora_core::asset::{AssetDecoding, AssetUUID, DecodeRequest, Decoded};
use khora_core::event::{BetweenSessions, Channel, Mailbox, Reader, WhenFull};
use khora_core::gpu::DeviceCaps;
use khora_core::lane::family::LaneFamily;
use khora_core::lane::guard::{SinkKind, SourceKind};
use khora_core::lane::Reserve;
use khora_core::Runtime;
use khora_data::ecs::World;
use khora_lanes::decode_lane::DecodeFamily;

/// The decode family's agent, by name.
const DECODE: AgentId = AgentId::new("decode");

/// How long a test waits for the scheduler to run the background.
const SETTLE: Duration = Duration::from_secs(5);

// ─── Fixtures ───────────────────────────────────────────────────────────────

/// The decode family's registration, as the engine collects it.
fn decode_registration() -> &'static AgentRegistration {
    let found: Vec<&'static AgentRegistration> = registered_agents()
        .into_iter()
        .filter(|registration| registration.family == TypeId::of::<DecodeFamily>())
        .collect();
    match found.as_slice() {
        [one] => one,
        [] => panic!(
            "the decode family is not registered: no registration of `DecodeFamily` was submitted"
        ),
        many => panic!("the decode family is registered {} times", many.len()),
    }
}

/// A decoder answering every request with the asset's uuid as its value,
/// remembering the thread it decoded on.
#[derive(Default)]
struct EchoDecoding {
    threads: Mutex<Vec<std::thread::ThreadId>>,
}

impl AssetDecoding for EchoDecoding {
    fn decode(&self, uuid: &AssetUUID, type_id: TypeId) -> Decoded {
        self.threads
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(std::thread::current().id());
        Decoded {
            uuid: *uuid,
            type_id,
            value: Ok(Box::new(*uuid) as Box<dyn Any + Send>),
            external_files: Vec::new(),
        }
    }
}

/// A runtime as the engine wires the decode family: the request channel,
/// the decoder, the reserve — and `activation`, when given.
struct Wired {
    runtime: Arc<Runtime>,
    requests: Channel<DecodeRequest>,
    reserve: Reserve,
    decoding: Arc<EchoDecoding>,
}

fn wired(activation: Option<Arc<Activation>>) -> Wired {
    let requests = Channel::<DecodeRequest>::bounded(64, WhenFull::DropOldest);
    let reserve = Reserve::new();
    let decoding = Arc::new(EchoDecoding::default());
    let mut runtime = Runtime::new();
    runtime.resources.insert(requests.clone());
    runtime.resources.insert(reserve.clone());
    runtime
        .resources
        .insert(Arc::clone(&decoding) as Arc<dyn AssetDecoding>);
    if let Some(activation) = activation {
        runtime.resources.insert(activation);
    }
    Wired {
        runtime: Arc::new(runtime),
        requests,
        reserve,
        decoding,
    }
}

fn request(path: &str) -> DecodeRequest {
    DecodeRequest {
        uuid: AssetUUID::new_v5(path),
        type_id: TypeId::of::<AssetUUID>(),
        generation: 0,
    }
}

/// The decode family's agent under the real scheduler.
fn scheduler_of(registration: &'static AgentRegistration) -> ExecutionScheduler {
    let agent: Arc<Mutex<dyn Agent>> =
        Arc::new(Mutex::new(lane_agent(registration, &DeviceCaps::default())));
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    registry
        .lock()
        .expect("the registry lock")
        .register(agent, 1.0);
    ExecutionScheduler::from_registry(registry, Arc::new(RwLock::new(Context::default())))
}

/// Whole frames, as the host runs them — the frame start, then the rest —
/// until a frame start brings decodes into the reserve: they are taken and
/// returned. Panics after [`SETTLE`].
fn frames_until_decoded(
    scheduler: &mut ExecutionScheduler,
    world: &mut World,
    wired: &Wired,
) -> Vec<Decoded> {
    let deadline = Instant::now() + SETTLE;
    loop {
        scheduler.begin_frame(&wired.runtime);
        let taken = wired.reserve.take::<Decoded>();
        if !taken.is_empty() {
            return taken;
        }
        scheduler.run_frame(world, Arc::clone(&wired.runtime));
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the decode to reach the reserve"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

// ─── The registration ───────────────────────────────────────────────────────

/// **The decode family registers once, at the background**: its own agent
/// (`decode`), optional, with one lane, the decode lane — written in
/// `khora-agents`' `decode_agent/agent.rs`.
#[test]
fn the_decode_family_is_registered_once_at_the_background() {
    let registration = decode_registration();

    assert_eq!(registration.family_name, "decode");
    assert_eq!(registration.agent, DECODE);
    assert_eq!(DecodeFamily::AGENT, DECODE);
    assert_eq!(registration.phase, ExecutionPhase::BACKGROUND);
    assert_eq!(registration.importance, AgentImportance::Optional);
    assert_eq!(registration.fixed_step, None);
    assert!(
        registration.module.ends_with("decode_agent::agent"),
        "registered in `decode_agent/agent.rs`: {}",
        registration.module
    );
    let lanes: Vec<&'static str> = registration
        .lanes
        .iter()
        .map(|make| make().name())
        .collect();
    assert_eq!(lanes.len(), 1, "one lane: {lanes:?}");
    assert!(
        lanes[0].ends_with("DecodeLane"),
        "the decode lane: {lanes:?}"
    );
}

/// **The decode lane reads only its requests, its decoder and its budget,
/// and only stages what it makes**: no `World`, no bus, no lock, no deck —
/// what a lane spanning frames may hold.
#[test]
fn the_decode_lane_reads_its_requests_its_decoder_and_its_budget_and_stages() {
    let lane = (decode_registration().lanes[0])();
    let declaration = lane.declaration();

    let inputs: Vec<(SourceKind, TypeId)> = declaration
        .inputs()
        .iter()
        .map(|input| (input.kind, input.ty))
        .collect();
    assert!(
        inputs.iter().all(|(kind, _)| matches!(
            kind,
            SourceKind::Take | SourceKind::Resource | SourceKind::Budget
        )),
        "only takes, resources and the budget: {:?}",
        declaration.inputs()
    );
    assert!(
        inputs.contains(&(SourceKind::Take, TypeId::of::<Channel<DecodeRequest>>())),
        "the requests, taken: {:?}",
        declaration.inputs()
    );
    assert!(
        inputs.contains(&(SourceKind::Resource, TypeId::of::<Arc<dyn AssetDecoding>>())),
        "the decoder, read: {:?}",
        declaration.inputs()
    );
    let outputs: Vec<(SinkKind, TypeId)> = declaration
        .outputs()
        .iter()
        .map(|output| (output.kind, output.ty))
        .collect();
    assert_eq!(
        outputs,
        vec![(SinkKind::Staged, TypeId::of::<Decoded>())],
        "the decoded assets, staged: {:?}",
        declaration.outputs()
    );
}

/// **The decode family's requests are taken from a mailbox it holds
/// between sessions**: an asset asked for while the family is off is decoded
/// when it comes back on — its request is never lost, so the asset service,
/// which files it once, never waits for it forever.
#[test]
fn the_decode_familys_request_mailbox_is_taken_and_held() {
    let found: Vec<&Mailbox> = khora_script::inventory::iter::<Mailbox>
        .into_iter()
        .filter(|mailbox| (mailbox.message)().ends_with("::DecodeRequest"))
        .collect();

    assert_eq!(found.len(), 1, "one mailbox of `DecodeRequest`: {found:?}");
    assert_eq!(
        (found[0].family, found[0].reader, found[0].between_sessions),
        (DECODE, Reader::Take, BetweenSessions::Hold)
    );
}

// ─── Boot ───────────────────────────────────────────────────────────────────

/// **Boot accepts the decode family** on a runtime holding its request
/// channel and its decoder — the background's rules among the checks — and
/// builds one agent from it.
#[test]
fn boot_accepts_the_decode_family() {
    let wired = wired(None);
    let registration = decode_registration();

    assert_eq!(
        check_registrations(&[registration], &wired.runtime, &DeviceCaps::default()),
        Ok(())
    );
    let agents = lane_agents(&[registration], &wired.runtime, &DeviceCaps::default())
        .expect("the decode family boots");
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].id(), DECODE);
}

// ─── Under the scheduler ────────────────────────────────────────────────────

/// **A filed request is decoded off the frame, and lands in the reserve at
/// a frame start.** The decode family's agent under the real scheduler: a
/// request filed before the first frame is taken, decoded by the runtime's
/// decoder on another thread than the one running the frames, and its
/// result reaches the reserve within a bounded number of frames — the
/// request channel left empty.
#[test]
fn a_filed_request_is_decoded_off_the_frame_into_the_reserve() {
    let wired = wired(None);
    let mut scheduler = scheduler_of(decode_registration());
    let mut world = World::new();
    wired.requests.send(request("meshes/hero.obj"));

    let decoded = frames_until_decoded(&mut scheduler, &mut world, &wired);

    assert_eq!(decoded.len(), 1, "one result: {decoded:?}");
    assert_eq!(decoded[0].uuid, AssetUUID::new_v5("meshes/hero.obj"));
    assert_eq!(
        decoded[0]
            .value
            .as_ref()
            .ok()
            .and_then(|value| value.downcast_ref::<AssetUUID>())
            .copied(),
        Some(AssetUUID::new_v5("meshes/hero.obj"))
    );
    assert!(wired.requests.is_empty(), "the request was taken");
    let frame_thread = std::thread::current().id();
    let threads = wired
        .decoding
        .threads
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    assert!(
        !threads.is_empty() && threads.iter().all(|thread| *thread != frame_thread),
        "decoded off the thread running the frames: {threads:?}"
    );
}

/// **The decode family runs while the scripting family is off**: an editor
/// editing a scene runs no script, yet its assets decode.
#[test]
fn a_request_is_decoded_while_the_scripting_family_is_off() {
    let activation = Arc::new(Activation::new([DECODE, khora_core::script::SCRIPT]));
    activation
        .deactivate(khora_core::script::SCRIPT)
        .expect("a known family");
    activation.settle();
    let wired = wired(Some(Arc::clone(&activation)));
    let mut scheduler = scheduler_of(decode_registration());
    let mut world = World::new();
    wired.requests.send(request("textures/albedo.png"));

    let decoded = frames_until_decoded(&mut scheduler, &mut world, &wired);

    assert!(
        !activation.is_active(khora_core::script::SCRIPT),
        "the premise: the scripts stayed off"
    );
    assert_eq!(decoded.len(), 1, "decoded while the scripts were off");
}

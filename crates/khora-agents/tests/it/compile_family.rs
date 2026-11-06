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

//! The compile family, registered and booted as the engine boots it: a
//! family of its own at `ExecutionPhase::BACKGROUND`, whose lane takes the
//! requests the hot-reload pump files and stages what the runtime's
//! `ScriptCompiler` returns — off the frame, and whether or not the
//! scripting family is on.

use std::any::TypeId;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use khora_agents::agent::lane_agent;
use khora_agents::registered_agent::{lane_agents, registered_agents};
use khora_control::{AgentRegistry, Context, ExecutionScheduler};
use khora_core::agent::gorna::AgentId;
use khora_core::agent::registration::{check_registrations, AgentRegistration};
use khora_core::agent::{Activation, Agent, AgentImportance, ExecutionPhase};
use khora_core::event::{BetweenSessions, Channel, Mailbox, Reader, WhenFull};
use khora_core::gpu::DeviceCaps;
use khora_core::lane::family::LaneFamily;
use khora_core::lane::guard::{SinkKind, SourceKind};
use khora_core::lane::Reserve;
use khora_core::Runtime;
use khora_data::ecs::World;
use khora_lanes::compile_lane::CompileFamily;
use khora_script::reload::ScriptReload;
use khora_script::toolchain::{CompileRequest, Compiled, ScriptCompiler, ScriptDeclarations};
use khora_script::vm::Program;

/// The compile family's agent, by name.
const COMPILE: AgentId = AgentId::new("compile");

/// How long a test waits for the scheduler to run the background.
const SETTLE: Duration = Duration::from_secs(5);

// ─── Fixtures ───────────────────────────────────────────────────────────────

/// The compile family's registration, as the engine collects it.
fn compile_registration() -> &'static AgentRegistration {
    let found: Vec<&'static AgentRegistration> = registered_agents()
        .into_iter()
        .filter(|registration| registration.family == TypeId::of::<CompileFamily>())
        .collect();
    match found.as_slice() {
        [one] => one,
        [] => panic!(
            "the compile family is not registered: no registration of `CompileFamily` was submitted"
        ),
        many => panic!("the compile family is registered {} times", many.len()),
    }
}

/// A compiler answering every request with a program for each module it
/// names, and declaring nothing.
#[derive(Default)]
struct EchoCompiler {
    asked: Mutex<Vec<CompileRequest>>,
}

impl ScriptCompiler for EchoCompiler {
    fn compile(&self, request: &CompileRequest) -> Compiled {
        self.asked
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(request.clone());
        Compiled {
            reloads: request
                .modules
                .iter()
                .map(|module| ScriptReload {
                    module: module.clone(),
                    program: Some(Program::default()),
                })
                .collect(),
            declarations: request
                .modules
                .iter()
                .map(|module| ScriptDeclarations {
                    module: module.clone(),
                    components: Vec::new(),
                })
                .collect(),
        }
    }
}

/// A runtime as the engine wires the compile family: the request channel,
/// the compiler, the reserve — and `activation`, when given.
struct Wired {
    runtime: Arc<Runtime>,
    requests: Channel<CompileRequest>,
    reserve: Reserve,
}

fn wired(activation: Option<Arc<Activation>>) -> Wired {
    let requests = Channel::<CompileRequest>::bounded(64, WhenFull::DropOldest);
    let reserve = Reserve::new();
    let mut runtime = Runtime::new();
    runtime.resources.insert(requests.clone());
    runtime.resources.insert(reserve.clone());
    runtime
        .resources
        .insert(Arc::new(EchoCompiler::default()) as Arc<dyn ScriptCompiler>);
    if let Some(activation) = activation {
        runtime.resources.insert(activation);
    }
    Wired {
        runtime: Arc::new(runtime),
        requests,
        reserve,
    }
}

fn request(root: &str, module: &str) -> CompileRequest {
    CompileRequest {
        root: PathBuf::from(root),
        modules: [module.to_owned()].into_iter().collect(),
    }
}

/// The compile family's agent under the real scheduler.
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
/// until a frame start brings reloads into the reserve: they are taken and
/// returned. Panics after [`SETTLE`].
fn frames_until_reloads(
    scheduler: &mut ExecutionScheduler,
    world: &mut World,
    wired: &Wired,
) -> Vec<ScriptReload> {
    let deadline = Instant::now() + SETTLE;
    loop {
        scheduler.begin_frame(&wired.runtime);
        let taken = wired.reserve.take::<ScriptReload>();
        if !taken.is_empty() {
            return taken;
        }
        scheduler.run_frame(world, Arc::clone(&wired.runtime));
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the compile to reach the reserve"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

// ─── The registration ───────────────────────────────────────────────────────

/// **The compile family registers once, at the background**: its own agent
/// (`compile`, not the scripting family's), optional, every run on request,
/// with one lane, the compile lane — written in `khora-agents`'
/// `compile_agent/agent.rs`.
#[test]
fn the_compile_family_is_registered_once_at_the_background() {
    let registration = compile_registration();

    assert_eq!(registration.family_name, "compile");
    assert_eq!(registration.agent, COMPILE);
    assert_eq!(CompileFamily::AGENT, COMPILE);
    assert_ne!(
        registration.agent,
        khora_core::script::SCRIPT,
        "a family of its own: it runs while the scripts are off"
    );
    assert_eq!(registration.phase, ExecutionPhase::BACKGROUND);
    assert_eq!(registration.importance, AgentImportance::Optional);
    assert_eq!(registration.fixed_step, None);
    assert!(
        registration.module.ends_with("compile_agent::agent"),
        "registered in `compile_agent/agent.rs`: {}",
        registration.module
    );
    let lanes: Vec<&'static str> = registration
        .lanes
        .iter()
        .map(|make| make().name())
        .collect();
    assert_eq!(lanes.len(), 1, "one lane: {lanes:?}");
    assert!(
        lanes[0].ends_with("CompileLane"),
        "the compile lane: {lanes:?}"
    );
}

/// **The compile lane reads only its requests, its compiler and its budget,
/// and only stages what it makes**: no `World`, no bus, no lock, no deck —
/// what a lane spanning frames may hold.
#[test]
fn the_compile_lane_reads_its_requests_its_compiler_and_its_budget_and_stages() {
    let lane = (compile_registration().lanes[0])();
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
        inputs.contains(&(SourceKind::Take, TypeId::of::<Channel<CompileRequest>>())),
        "the requests, taken: {:?}",
        declaration.inputs()
    );
    assert!(
        inputs.contains(&(
            SourceKind::Resource,
            TypeId::of::<Arc<dyn ScriptCompiler>>()
        )),
        "the compiler, read: {:?}",
        declaration.inputs()
    );
    let outputs: Vec<(SinkKind, TypeId)> = declaration
        .outputs()
        .iter()
        .map(|output| (output.kind, output.ty))
        .collect();
    assert_eq!(
        outputs.len(),
        2,
        "the reloads and the declarations: {:?}",
        declaration.outputs()
    );
    assert!(outputs.contains(&(SinkKind::Staged, TypeId::of::<ScriptReload>())));
    assert!(outputs.contains(&(SinkKind::Staged, TypeId::of::<ScriptDeclarations>())));
}

/// **The compile family's requests are taken from a mailbox it holds
/// between sessions**: a save filed while the family is off is compiled when
/// it comes back on.
#[test]
fn the_compile_familys_request_mailbox_is_taken_and_held() {
    let found: Vec<&Mailbox> = khora_script::inventory::iter::<Mailbox>
        .into_iter()
        .filter(|mailbox| (mailbox.message)().ends_with("::CompileRequest"))
        .collect();

    assert_eq!(found.len(), 1, "one mailbox of `CompileRequest`: {found:?}");
    assert_eq!(
        (found[0].family, found[0].reader, found[0].between_sessions),
        (COMPILE, Reader::Take, BetweenSessions::Hold)
    );
}

// ─── Boot ───────────────────────────────────────────────────────────────────

/// **Boot accepts the compile family** on a runtime holding its request
/// channel and its compiler — the background's rules among the checks — and
/// builds one agent from it.
#[test]
fn boot_accepts_the_compile_family() {
    let wired = wired(None);
    let registration = compile_registration();

    assert_eq!(
        check_registrations(&[registration], &wired.runtime, &DeviceCaps::default()),
        Ok(())
    );
    let agents = lane_agents(&[registration], &wired.runtime, &DeviceCaps::default())
        .expect("the compile family boots");
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].id(), COMPILE);
}

// ─── Under the scheduler ────────────────────────────────────────────────────

/// **A filed request is compiled off the frame, and lands in the reserve at
/// a frame start.** The compile family's agent under the real scheduler: a
/// request filed before the first frame is taken, compiled by the runtime's
/// compiler, and its reload reaches the reserve within a bounded number of
/// frames — the request channel left empty.
#[test]
fn a_filed_request_is_compiled_off_the_frame_into_the_reserve() {
    let wired = wired(None);
    let mut scheduler = scheduler_of(compile_registration());
    let mut world = World::new();
    wired.requests.send(request("assets", "ai/guard.erg"));

    let reloads = frames_until_reloads(&mut scheduler, &mut world, &wired);

    assert_eq!(
        reloads,
        vec![ScriptReload {
            module: "ai/guard.erg".to_owned(),
            program: Some(Program::default()),
        }]
    );
    assert_eq!(
        wired
            .reserve
            .take::<ScriptDeclarations>()
            .into_iter()
            .map(|set| set.module)
            .collect::<Vec<_>>(),
        vec!["ai/guard.erg".to_owned()],
        "the declarations landed beside the reload"
    );
    assert!(wired.requests.is_empty(), "the request was taken");
}

/// **The compile family runs while the scripting family is off**: an editor
/// editing a scene runs no script, yet a save is compiled and waits in the
/// reserve for the scripts to come back.
#[test]
fn a_request_is_compiled_while_the_scripting_family_is_off() {
    let activation = Arc::new(Activation::new([COMPILE, khora_core::script::SCRIPT]));
    activation
        .deactivate(khora_core::script::SCRIPT)
        .expect("a known family");
    activation.settle();
    let wired = wired(Some(Arc::clone(&activation)));
    let mut scheduler = scheduler_of(compile_registration());
    let mut world = World::new();
    wired.requests.send(request("assets", "ai/guard.erg"));

    let reloads = frames_until_reloads(&mut scheduler, &mut world, &wired);

    assert!(
        !activation.is_active(khora_core::script::SCRIPT),
        "the premise: the scripts stayed off"
    );
    assert_eq!(reloads.len(), 1, "compiled while the scripts were off");
}

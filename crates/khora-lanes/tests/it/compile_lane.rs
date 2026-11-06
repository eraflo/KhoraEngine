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

//! **The compile lane hands the requests it takes to the runtime's compiler,
//! and stages what comes back.** Run by hand as the scheduler runs a
//! background lane: its inputs fetched from a runtime holding the request
//! channel and a fake [`ScriptCompiler`], its outputs committed into a
//! staging the reserve then receives. It reads no `World`, no bus, and locks
//! nothing.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use khora_core::event::{Channel, WhenFull};
use khora_core::lane::contract::Lane;
use khora_core::lane::io::{FrameId, InputSource, LaneInputs, LaneOutputs, OutputSink};
use khora_core::lane::{LaneBus, OutputDeck, Reserve, Staging};
use khora_core::Runtime;
use khora_lanes::compile_lane::{CompileInputs, CompileLane, CompileOutputs};
use khora_script::reload::ScriptReload;
use khora_script::toolchain::{CompileRequest, Compiled, ScriptCompiler, ScriptDeclarations};
use khora_script::vm::Program;

/// A compiler that answers every request with what it was given, and
/// remembers the requests, in the order it was asked.
#[derive(Default)]
struct FakeCompiler {
    answer: Compiled,
    asked: Mutex<Vec<CompileRequest>>,
}

impl FakeCompiler {
    fn answering(answer: Compiled) -> Arc<Self> {
        Arc::new(Self {
            answer,
            asked: Mutex::new(Vec::new()),
        })
    }

    fn asked(&self) -> Vec<CompileRequest> {
        self.asked
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

impl ScriptCompiler for FakeCompiler {
    fn compile(&self, request: &CompileRequest) -> Compiled {
        self.asked
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(request.clone());
        self.answer.clone()
    }
}

fn request(root: &str, modules: &[&str]) -> CompileRequest {
    CompileRequest {
        root: PathBuf::from(root),
        modules: modules.iter().map(|module| (*module).to_owned()).collect(),
    }
}

fn reload(module: &str) -> ScriptReload {
    ScriptReload {
        module: module.to_owned(),
        program: Some(Program::default()),
    }
}

fn declarations(module: &str) -> ScriptDeclarations {
    ScriptDeclarations {
        module: module.to_owned(),
        components: Vec::new(),
    }
}

/// A runtime holding the request channel, filled with `requests`, and
/// `compiler` as the engine installs it.
fn runtime_with(requests: &[CompileRequest], compiler: Arc<FakeCompiler>) -> Runtime {
    let channel = Channel::<CompileRequest>::bounded(64, WhenFull::Reject);
    for request in requests {
        channel.send(request.clone());
    }
    let mut runtime = Runtime::new();
    runtime.resources.insert(channel);
    runtime
        .resources
        .insert(compiler as Arc<dyn ScriptCompiler>);
    runtime
}

/// One run of the lane over `runtime`, as the scheduler runs it at the
/// background: its inputs fetched, the lane recorded, its outputs committed
/// into a staging the returned reserve received.
fn run(runtime: &Runtime) -> Reserve {
    let bus = LaneBus::new();
    let mut source = InputSource::new(FrameId(0), &bus, runtime);
    let mut inputs = CompileInputs::fetch(&mut source).expect("the compile lane's inputs");
    let mut deck = OutputDeck::new();
    let mut staging = Staging::new();
    {
        let mut sink = OutputSink::new(&mut deck).with_staging(&mut staging);
        let mut outputs = CompileOutputs::open(&mut sink).expect("the compile lane's outputs");
        CompileLane
            .record(0, &mut inputs, &mut outputs)
            .expect("the compile runs");
        outputs.commit(&mut sink).expect("the outputs are staged");
    }
    let reserve = Reserve::new();
    reserve.receive(staging);
    reserve
}

/// **A run stages the reloads and the declarations the compiler returns.**
/// One request waits; the fake compiler answers with a reload and a set of
/// declarations: both reach the reserve, and the request channel is left
/// empty.
#[test]
fn a_run_stages_what_the_compiler_returns() {
    let compiler = FakeCompiler::answering(Compiled {
        reloads: vec![reload("ai/guard.erg")],
        declarations: vec![declarations("ai/guard.erg")],
    });
    let runtime = runtime_with(
        &[request("assets", &["ai/guard.erg"])],
        Arc::clone(&compiler),
    );

    let reserve = run(&runtime);

    assert_eq!(compiler.asked(), vec![request("assets", &["ai/guard.erg"])]);
    assert_eq!(
        reserve.take::<ScriptReload>(),
        vec![reload("ai/guard.erg")],
        "the reload is staged"
    );
    assert_eq!(
        reserve.take::<ScriptDeclarations>(),
        vec![declarations("ai/guard.erg")],
        "the declarations are staged"
    );
    assert!(
        runtime
            .resources
            .get::<Channel<CompileRequest>>()
            .is_some_and(|channel| channel.is_empty()),
        "the request was taken"
    );
}

/// **The requests of one root are compiled once, their modules merged.**
/// Two requests of `game` wait beside one of `editor`: the compiler is asked
/// once per root, `game` with both its modules, in the order the roots
/// first asked.
#[test]
fn the_requests_of_one_root_are_compiled_once_with_their_modules_merged() {
    let compiler = FakeCompiler::answering(Compiled::default());
    let runtime = runtime_with(
        &[
            request("game", &["a.erg"]),
            request("editor", &["x.erg"]),
            request("game", &["b.erg"]),
        ],
        Arc::clone(&compiler),
    );

    let _ = run(&runtime);

    assert_eq!(
        compiler.asked(),
        vec![
            request("game", &["a.erg", "b.erg"]),
            request("editor", &["x.erg"]),
        ]
    );
}

/// **What every compile returns is staged, in the order the roots were
/// compiled.** Each root's answer reaches the reserve: none is lost to the
/// next compile of the run.
#[test]
fn what_every_compile_of_a_run_returns_is_staged() {
    let compiler = FakeCompiler::answering(Compiled {
        reloads: vec![reload("shared.erg")],
        declarations: Vec::new(),
    });
    let runtime = runtime_with(
        &[request("game", &["a.erg"]), request("editor", &["x.erg"])],
        Arc::clone(&compiler),
    );

    let reserve = run(&runtime);

    assert_eq!(compiler.asked().len(), 2, "one compile per root");
    // The reserve keeps one reload per module: the second compile's reload
    // of `shared.erg` replaced the first's, which never reached a boundary.
    assert_eq!(reserve.take::<ScriptReload>(), vec![reload("shared.erg")]);
}

/// **A run with no request compiles nothing and stages nothing.**
#[test]
fn a_run_without_a_request_compiles_nothing() {
    let compiler = FakeCompiler::answering(Compiled {
        reloads: vec![reload("ai/guard.erg")],
        declarations: vec![declarations("ai/guard.erg")],
    });
    let runtime = runtime_with(&[], Arc::clone(&compiler));

    let reserve = run(&runtime);

    assert!(compiler.asked().is_empty(), "the compiler was never asked");
    assert!(reserve.is_empty::<ScriptReload>());
    assert!(reserve.is_empty::<ScriptDeclarations>());
}

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

//! What the hot-reload pump files, compiled as the compile family compiles
//! it — in place, for these tests: khora-io holds the pump and the
//! [`ProjectCompiler`], not the family's lane. Each request is compiled by a
//! `ProjectCompiler` over the runtime's resources, and what comes back is
//! staged into the runtime's reserve, as a frame start hands it over.

use khora_core::event::{Channel, WhenFull};
use khora_core::lane::{OutputDeck, Reserve, Staging};
use khora_core::Runtime;
use khora_data::ecs::{DataSystemRegistration, SystemContext, Tick, World};
use khora_io::script::project_compiler::ProjectCompiler;
use khora_script::reload::ScriptReload;
use khora_script::toolchain::{CompileRequest, Compiled, ScriptCompiler, ScriptDeclarations};

/// Puts in `runtime` what the pump files into and what the compile stages
/// into: the request channel and the reserve.
pub fn install(runtime: &mut Runtime) {
    runtime.resources.insert(Channel::<CompileRequest>::bounded(
        1024,
        WhenFull::DropOldest,
    ));
    runtime.resources.insert(Reserve::new());
}

/// The registered `script_hot_reload` pump.
pub fn pump() -> &'static DataSystemRegistration {
    inventory::iter::<DataSystemRegistration>
        .into_iter()
        .find(|system| system.name == "script_hot_reload")
        .expect("the pump is registered")
}

/// One run of the pump over `runtime`, alone: what it files stays filed.
pub fn run_pump_alone(world: &mut World, runtime: &Runtime) {
    (pump().run)(
        world,
        runtime,
        &mut OutputDeck::new(),
        SystemContext {
            since: Tick(0),
            resumed: false,
            scope: None,
        },
    );
}

/// One run of the pump over `runtime`, then every request it filed
/// compiled and staged. Returns how many requests were compiled.
pub fn run_pump(world: &mut World, runtime: &Runtime) -> usize {
    run_pump_alone(world, runtime);
    compile_filed(runtime)
}

/// The requests the pump filed and nothing took yet, taken.
pub fn filed(runtime: &Runtime) -> Vec<CompileRequest> {
    runtime
        .resources
        .get::<Channel<CompileRequest>>()
        .map(Channel::drain)
        .unwrap_or_default()
}

/// Every request filed in `runtime`, compiled by a [`ProjectCompiler`] over
/// the runtime's resources, and what each returned staged into the runtime's
/// reserve. Returns how many requests were compiled.
pub fn compile_filed(runtime: &Runtime) -> usize {
    let requests = filed(runtime);
    if requests.is_empty() {
        return 0;
    }
    let compiler = ProjectCompiler::from_runtime(runtime);
    for request in &requests {
        stage(runtime, compiler.compile(request));
    }
    requests.len()
}

/// Stages `compiled` into the runtime's reserve.
pub fn stage(runtime: &Runtime, compiled: Compiled) {
    let reserve = runtime
        .resources
        .get::<Reserve>()
        .expect("the runtime's reserve");
    stage_into(reserve, compiled);
}

/// Stages `compiled` into `reserve`, as a frame start hands a compile run's
/// staging over.
pub fn stage_into(reserve: &Reserve, compiled: Compiled) {
    let mut staging = Staging::new();
    staging.stage(compiled.reloads);
    staging.stage(compiled.declarations);
    reserve.receive(staging);
}

/// The reloads staged in the runtime's reserve, taken.
pub fn staged_reloads(runtime: &Runtime) -> Vec<ScriptReload> {
    runtime
        .resources
        .get::<Reserve>()
        .map(Reserve::take::<ScriptReload>)
        .unwrap_or_default()
}

/// The modules of the reloads staged in the runtime's reserve, taken.
pub fn staged_modules(runtime: &Runtime) -> Vec<String> {
    staged_reloads(runtime)
        .into_iter()
        .map(|reload| reload.module)
        .collect()
}

/// The declarations staged in the runtime's reserve, taken.
pub fn staged_declarations(runtime: &Runtime) -> Vec<ScriptDeclarations> {
    runtime
        .resources
        .get::<Reserve>()
        .map(Reserve::take::<ScriptDeclarations>)
        .unwrap_or_default()
}

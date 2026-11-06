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

//! The compile lane: the requests the pump filed, compiled by the runtime's
//! [`ScriptCompiler`].

use std::sync::Arc;
use std::time::Duration;

use khora_core::agent::gorna::StrategyId;
use khora_core::lane::cost::PerDevice;
use khora_core::lane::level::Impact;

use khora_core::lane::contract::{Lane, LaneReport};
use khora_core::lane::cost::Cost;
use khora_core::lane::io::Budget;
use khora_core::lane::level::Level;
use khora_core::lane::LaneError;
use khora_macros::LaneInputs;
use khora_script::toolchain::{CompileRequest, ScriptCompiler};

use super::{CompileFamily, CompileOutputs};

/// What a compile reads: the requests waiting, taken; the compiler; its
/// budget. Nothing of a frame — a run may span several.
#[derive(LaneInputs)]
pub struct CompileInputs<'f> {
    /// The requests the pump filed since the last run.
    #[take]
    pub requests: Vec<CompileRequest>,
    /// Who compiles them.
    #[resource]
    pub compiler: &'f Arc<dyn ScriptCompiler>,
    /// The time the run is lent.
    #[budget]
    pub budget: Option<Budget>,
}

/// Compiles the requests it takes, one compile per root with the modules of
/// that root's requests merged, and stages what the compiler returns.
#[derive(Debug, Default)]
pub struct CompileLane;

/// The one level: a compile never trades its result.
const LEVELS: [Level; 1] = [Level {
    id: StrategyId::Balanced,
    name: "compile",
    impact: Impact::None,
    memory: PerDevice {
        cpu: 0,
        gpu: 0,
        npu: 0,
        other: 0,
    },
}];

/// What compiling one module costs on the host, about, before the runs
/// measure it: a parse, a check and an emit of a few hundred lines.
const MODULE_COST: Duration = Duration::from_millis(2);

impl Lane for CompileLane {
    type Family = CompileFamily;
    type Inputs<'f> = CompileInputs<'f>;

    fn levels(&self) -> &[Level] {
        &LEVELS
    }

    fn cost(&self, _level: usize, inputs: &CompileInputs<'_>) -> Cost {
        let modules: usize = inputs
            .requests
            .iter()
            .map(|request| request.modules.len())
            .sum();
        Cost {
            time: PerDevice {
                cpu: MODULE_COST * u32::try_from(modules).unwrap_or(u32::MAX),
                ..PerDevice::default()
            },
            ..Cost::default()
        }
    }

    fn record(
        &mut self,
        _level: usize,
        inputs: &mut CompileInputs<'_>,
        out: &mut CompileOutputs,
    ) -> Result<LaneReport, LaneError> {
        // One compile per root, its requests' modules merged, in the order
        // the roots were first asked for.
        let mut by_root: Vec<CompileRequest> = Vec::new();
        for request in std::mem::take(&mut inputs.requests) {
            match by_root
                .iter_mut()
                .find(|merged| merged.root == request.root)
            {
                Some(merged) => merged.modules.extend(request.modules),
                None => by_root.push(request),
            }
        }
        // Each root's outputs under its own lease: a root closed while the
        // run is out drops what was compiled for it, and only that.
        for request in &by_root {
            let (compiled, lease) = inputs.compiler.compile_leased(request);
            out.reloads.push(compiled.reloads, lease.clone());
            out.declarations.push(compiled.declarations, lease);
        }
        Ok(LaneReport {
            done: 1.0,
            stalled: false,
            message: None,
        })
    }
}

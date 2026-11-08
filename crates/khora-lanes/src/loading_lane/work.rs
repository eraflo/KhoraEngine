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

//! What a unit of each kind of load costs and does, whichever lane loads it.
//!
//! A unit is one project index, one scene load, one script root — every
//! waiting request of that root, merged into one compile — or one asset
//! decode. The lanes differ only
//! in how many units a run takes.

use std::time::Duration;

use khora_core::asset::{Decoded, IndexedAssets};
use khora_core::lane::contract::LaneReport;
use khora_core::lane::cost::{Cost, PerDevice};
use khora_script::toolchain::{CompileRequest, Compiled};

use super::{LoadingInputs, LoadingOutputs};

/// What indexing one project is priced at before any is measured: a walk of
/// a few thousand files, the meshes among them read for their parts.
const INDEX_COST: Duration = Duration::from_millis(50);

/// What one scene load is priced at before any is measured: a scene of a few
/// thousand entities, read and staged.
const SCENE_COST: Duration = Duration::from_millis(20);

/// What compiling one module costs on the host, about, before the runs
/// measure it: a parse, a check and an emit of a few hundred lines.
const MODULE_COST: Duration = Duration::from_millis(2);

/// What one decode is priced at before any is measured: a texture or a mesh
/// of a few megabytes.
const DECODE_COST: Duration = Duration::from_millis(4);

/// How many units of each kind a run takes, from the front of each field.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Taking {
    /// The first project indexes.
    pub(super) indexes: usize,
    /// The first scene loads.
    pub(super) scenes: usize,
    /// The first script roots, in the order each was first filed.
    pub(super) roots: usize,
    /// The first asset decodes.
    pub(super) assets: usize,
}

/// The work waiting in `inputs`, by kind and unit.
pub(super) struct Waiting {
    /// Each project index's cost, in filing order.
    indexes: Vec<Duration>,
    /// Each scene load's cost, in filing order.
    scenes: Vec<Duration>,
    /// Each script root's cost, in the order each was first filed.
    roots: Vec<Duration>,
    /// Each asset decode's cost, in filing order.
    assets: Vec<Duration>,
}

impl Waiting {
    /// The units waiting in `inputs`.
    pub(super) fn of(inputs: &LoadingInputs<'_>) -> Self {
        let roots = roots(&inputs.scripts)
            .iter()
            .map(|requests| {
                let modules: usize = requests
                    .iter()
                    .map(|request| inputs.scripts[*request].modules.len())
                    .sum();
                MODULE_COST * u32::try_from(modules).unwrap_or(u32::MAX)
            })
            .collect();
        Self {
            indexes: vec![INDEX_COST; inputs.indexes.len()],
            scenes: vec![SCENE_COST; inputs.scenes.len()],
            roots,
            assets: vec![DECODE_COST; inputs.assets.len()],
        }
    }

    /// Every unit.
    pub(super) fn all(&self) -> Taking {
        Taking {
            indexes: self.indexes.len(),
            scenes: self.scenes.len(),
            roots: self.roots.len(),
            assets: self.assets.len(),
        }
    }

    /// The units taken in turn — the project indexes, which the scenes and the
    /// assets are read through, then the scenes, the script roots and the
    /// assets — while their cost stays within `budget`, and always the first;
    /// everything without a budget. The first unit that does not fit stops
    /// the taking, so what is left keeps its order.
    pub(super) fn within(&self, budget: Option<Duration>) -> Taking {
        let Some(budget) = budget else {
            return self.all();
        };
        let mut taken = [0usize; 4];
        let mut spent = Duration::ZERO;
        let kinds = [&self.indexes, &self.scenes, &self.roots, &self.assets];
        'taking: for (kind, costs) in kinds.into_iter().enumerate() {
            for cost in costs {
                let first = taken.iter().all(|count| *count == 0);
                if !first && spent + *cost > budget {
                    break 'taking;
                }
                spent += *cost;
                taken[kind] += 1;
            }
        }
        Taking {
            indexes: taken[0],
            scenes: taken[1],
            roots: taken[2],
            assets: taken[3],
        }
    }

    /// What taking `taking` costs, and the share of the waiting work it is:
    /// no share when it is all of it.
    pub(super) fn cost(&self, taking: Taking) -> Cost {
        let total = self.time(self.all());
        let time = self.time(taking);
        let share = (time < total).then(|| (time.as_secs_f64() / total.as_secs_f64()) as f32);
        Cost {
            time: PerDevice {
                cpu: time,
                ..PerDevice::default()
            },
            share,
            ..Cost::default()
        }
    }

    /// The time `taking` costs.
    fn time(&self, taking: Taking) -> Duration {
        self.indexes[..taking.indexes].iter().sum::<Duration>()
            + self.scenes[..taking.scenes].iter().sum::<Duration>()
            + self.roots[..taking.roots].iter().sum::<Duration>()
            + self.assets[..taking.assets].iter().sum::<Duration>()
    }
}

/// Runs one unit's load, turning a panic into the message it carried.
fn catch_unit<R>(unit: impl FnOnce() -> R) -> Result<R, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(unit)).map_err(|panic| {
        panic
            .downcast_ref::<&str>()
            .map(|why| (*why).to_owned())
            .or_else(|| panic.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "no message".to_owned())
    })
}

/// The script requests of each root, by index into `scripts`, the roots in
/// the order each was first filed.
fn roots(scripts: &[CompileRequest]) -> Vec<Vec<usize>> {
    let mut roots: Vec<Vec<usize>> = Vec::new();
    for (index, request) in scripts.iter().enumerate() {
        match roots
            .iter_mut()
            .find(|requests| scripts[requests[0]].root == request.root)
        {
            Some(requests) => requests.push(index),
            None => roots.push(vec![index]),
        }
    }
    roots
}

/// Loads the units `taking` names from the front of each field, staging each
/// result under its lease; what it does not take stays in its field, in
/// order, and goes back to its mailbox after the run.
pub(super) fn load(
    taking: Taking,
    inputs: &mut LoadingInputs<'_>,
    out: &mut LoadingOutputs,
) -> LaneReport {
    // The project indexes in filing order, each under its own lease: a
    // project replaced while the run is out drops its index, and only it.
    let indexes: Vec<_> = inputs.indexes.drain(..taking.indexes).collect();
    for request in indexes {
        let (indexed, lease) = catch_unit(|| inputs.indexing.index_leased(&request))
            .unwrap_or_else(|why| {
                let indexed = IndexedAssets {
                    ticket: request.ticket,
                    kind: request.kind,
                    project_root: request.project_root.clone(),
                    index: Err(format!("indexing the project panicked: {why}")),
                };
                (indexed, None)
            });
        out.indexed.push(vec![indexed], lease);
    }

    // The scenes in filing order, each under its own lease: the boundary
    // commits them in that order.
    let scenes: Vec<_> = inputs.scenes.drain(..taking.scenes).collect();
    for request in scenes {
        let (prepared, lease) = inputs.loading.prepare_leased(request);
        out.prepared.push(vec![prepared], lease);
    }

    // One compile per root taken, its requests' modules merged, in the order
    // the roots were first filed; the requests of the roots left stay.
    let taken_roots: Vec<Vec<usize>> = roots(&inputs.scripts)
        .into_iter()
        .take(taking.roots)
        .collect();
    let mut merged: Vec<CompileRequest> = Vec::with_capacity(taken_roots.len());
    let mut left = Vec::new();
    for (index, request) in std::mem::take(&mut inputs.scripts).into_iter().enumerate() {
        if !taken_roots.iter().any(|requests| requests.contains(&index)) {
            left.push(request);
            continue;
        }
        match merged.iter_mut().find(|kept| kept.root == request.root) {
            Some(kept) => kept.modules.extend(request.modules),
            None => merged.push(request),
        }
    }
    inputs.scripts = left;
    // Each root's outputs under its own lease: a root closed while the run
    // is out drops what was compiled for it, and only that.
    for request in &merged {
        // A compiler that panics fails this root alone: the loads taken
        // beside it are staged all the same.
        let (compiled, lease) = catch_unit(|| inputs.compiler.compile_leased(request))
            .unwrap_or_else(|why| {
                log::error!(
                    "compiling the scripts of {} panicked: {why}",
                    request.root.display()
                );
                (Compiled::default(), None)
            });
        out.reloads.push(compiled.reloads, lease.clone());
        out.declarations.push(compiled.declarations, lease);
    }

    // Each decode under its own asset's lease: an asset changed while the
    // run is out drops its own result, and only it.
    let assets: Vec<_> = inputs.assets.drain(..taking.assets).collect();
    for request in assets {
        // A decoder that panics fails this asset alone: its failure is
        // staged, like any decode that could not be done.
        let (decoded, lease) = catch_unit(|| inputs.decoding.decode_leased(&request))
            .unwrap_or_else(|why| {
                let decoded = Decoded {
                    uuid: request.uuid,
                    type_id: request.type_id,
                    value: Err(format!("decoding panicked: {why}")),
                    external_files: Vec::new(),
                };
                (decoded, None)
            });
        out.decoded.push(vec![decoded], lease);
    }

    LaneReport {
        done: 1.0,
        stalled: false,
        message: None,
    }
}

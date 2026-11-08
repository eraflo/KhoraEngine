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

//! The toolchain's words: which request replaces a waiting one, and which
//! staged output replaces a waiting one in the reserve.

use std::collections::BTreeSet;
use std::path::PathBuf;

use khora_core::event::{Channel, Supersedes, WhenFull};
use khora_core::lane::{Reserve, Staging};

use super::{CompileRequest, CompileTicket, ScriptDeclarations};

fn request(root: &str, modules: &[&str]) -> CompileRequest {
    CompileRequest::new(
        PathBuf::from(root),
        modules.iter().map(|module| (*module).to_owned()),
    )
}

/// `request(root, modules)`, waited on by `tickets`.
fn ticketed(root: &str, modules: &[&str], tickets: &[u64]) -> CompileRequest {
    let mut request = request(root, modules);
    request.tickets = tickets.iter().copied().map(CompileTicket).collect();
    request
}

fn declarations(module: &str, count: usize) -> ScriptDeclarations {
    ScriptDeclarations {
        module: module.to_owned(),
        components: (0..count)
            .map(|at| crate::vm::DeclaredComponent {
                name: format!("C{at}"),
                module: module.to_owned(),
                domain: khora_core::ecs::SemanticDomain::Script,
                fields: Vec::new(),
                kind: crate::vm::DeclaredKind::Component,
                order: 0,
            })
            .collect(),
    }
}

// ─── CompileRequest ─────────────────────────────────────────────────────────

/// **The same save twice is one compile.** A request of a root naming every
/// module a waiting one names replaces it: nothing the waiting one asked is
/// lost.
#[test]
fn a_request_naming_every_module_of_a_waiting_one_of_its_root_replaces_it() {
    assert!(request("assets", &["a.erg"]).supersedes(&request("assets", &["a.erg"])));
    assert!(request("assets", &["a.erg", "b.erg"]).supersedes(&request("assets", &["a.erg"])));
}

/// **A request never drops a module a waiting one asked for.** A newer
/// request of the same root naming fewer or other modules does not replace
/// the waiting one: the two wait side by side, and the lane merges them.
#[test]
fn a_request_missing_a_module_of_a_waiting_one_never_replaces_it() {
    assert!(!request("assets", &["b.erg"]).supersedes(&request("assets", &["a.erg"])));
    assert!(!request("assets", &["a.erg"]).supersedes(&request("assets", &["a.erg", "b.erg"])));
}

/// **A request of one root never replaces another root's**, whatever it
/// names: the modules of two roots are two projects' files.
#[test]
fn a_request_of_another_root_never_replaces_a_waiting_one() {
    assert!(!request("game", &["a.erg"]).supersedes(&request("editor", &["a.erg"])));
}

/// **On the pump's channel, saves of one file coalesce and other files
/// wait beside them.** Three saves: `a`, `a` again, then `b` — two requests
/// wait, in order, and none names a module twice.
#[test]
fn repeated_saves_of_one_file_wait_as_one_request_on_the_channel() {
    let pending = Channel::bounded(16, WhenFull::DropOldest);
    pending.send(request("assets", &["a.erg"]));
    pending.send(request("assets", &["a.erg"]));
    pending.send(request("assets", &["b.erg"]));

    assert_eq!(
        pending.drain(),
        vec![request("assets", &["a.erg"]), request("assets", &["b.erg"])]
    );
}

/// **A hot-reload request never swallows a mount.** A request no one waits
/// on — the pump's — of the same root, naming every module a mount's request
/// names and more, does not replace it: the mount's ticket would be lost, and
/// its host would wait forever. A request carrying every ticket of the
/// waiting one replaces it; one carrying only some of them does not.
#[test]
fn a_hot_reload_request_never_swallows_a_mount() {
    let mount = ticketed("assets", &["a.erg"], &[1]);

    assert!(
        !request("assets", &["a.erg", "b.erg"]).supersedes(&mount),
        "a ticketless request swallowed a mount's ticket"
    );
    assert!(
        ticketed("assets", &["a.erg", "b.erg"], &[1]).supersedes(&mount),
        "a request carrying the mount's ticket covers it"
    );
    assert!(
        ticketed("assets", &["a.erg"], &[1, 2]).supersedes(&mount),
        "a request carrying more tickets covers it"
    );
    assert!(
        !ticketed("assets", &["a.erg", "b.erg"], &[2]).supersedes(&mount),
        "a request carrying another ticket swallowed the mount's"
    );
    assert!(
        !ticketed("assets", &["a.erg", "b.erg"], &[2]).supersedes(&ticketed(
            "assets",
            &["a.erg"],
            &[1, 2]
        )),
        "a request carrying only some of the waiting tickets swallowed the others"
    );
    assert!(
        ticketed("assets", &["a.erg"], &[1]).supersedes(&request("assets", &["a.erg"])),
        "a mount covers a waiting hot-reload request of its modules"
    );
}

/// **On the channel, a pump's save after a mount leaves the mount waiting.**
/// A mount of `a` and `b` waits; the pump files `a`, `b` and `c`: both wait,
/// the mount's ticket with them.
#[test]
fn a_pump_request_after_a_mount_waits_beside_it_on_the_channel() {
    let pending = Channel::bounded(16, WhenFull::Reject);
    pending.send(ticketed("assets", &["a.erg", "b.erg"], &[7]));
    pending.send(request("assets", &["a.erg", "b.erg", "c.erg"]));

    let waiting = pending.drain();
    assert_eq!(waiting.len(), 2, "{waiting:?}");
    assert!(
        waiting
            .iter()
            .any(|request| request.tickets.contains(&CompileTicket(7))),
        "the mount's ticket still waits: {waiting:?}"
    );
}

/// **A request is a value the take hands on whole**: its root and its
/// modules, sorted, each once.
#[test]
fn a_request_names_each_module_once_sorted() {
    let request = request("assets", &["b.erg", "a.erg", "b.erg"]);
    assert_eq!(
        request.modules,
        BTreeSet::from(["a.erg".to_owned(), "b.erg".to_owned()])
    );
    assert!(
        request.supersedes(&request.clone()),
        "a request covers itself"
    );
}

// ─── ScriptDeclarations, staged ─────────────────────────────────────────────

/// Stages `items` into `reserve`, as the scheduler hands a run's staging
/// over at a frame start.
fn stage(reserve: &Reserve, items: Vec<ScriptDeclarations>) {
    let mut staging = Staging::new();
    staging.stage(items);
    reserve.receive(staging);
}

/// **A module's newer declarations replace its waiting ones in the
/// reserve**: a boundary registers only what the file says now.
#[test]
fn a_newer_declaration_set_of_a_module_replaces_the_waiting_one() {
    let reserve = Reserve::new();
    stage(&reserve, vec![declarations("ai/guard.erg", 2)]);
    stage(&reserve, vec![declarations("ai/guard.erg", 0)]);

    assert_eq!(
        reserve.take::<ScriptDeclarations>(),
        vec![declarations("ai/guard.erg", 0)]
    );
}

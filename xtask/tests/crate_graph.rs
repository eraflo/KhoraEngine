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

//! The CLAD dependency direction (`.agent/engine/RULES.md` §3), read from the
//! manifests.
//!
//! The graph starts from the root manifest's `workspace.members` and follows
//! every `path` dependency, so a path crate that is not a member
//! (`khora-macros`) is part of it. An edge is a first-party crate listed under
//! `[dependencies]` or `[build-dependencies]`, target-specific tables
//! included, directly or through `workspace = true`: what ships. Dev
//! dependencies are left out — a test may reach any crate without the crate
//! under test depending on it. Edges are keyed by the target's package name,
//! read from its own manifest, so a `package = "…"` rename cannot hide one.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Component, Path, PathBuf};

use toml::{Table, Value};

/// Dependency tables whose entries ship with the crate.
const SHIPPED_TABLES: &[&str] = &["dependencies", "build-dependencies", "build_dependencies"];

/// Crates outside the CLAD tiers: build tooling, not engine or game code.
const UNTIERED: &[&str] = &["xtask"];

/// Crates that carry no code and are left out of the graph: the
/// `workspace-hack` cargo-hakari generates, which only unifies the features
/// of the third-party dependencies (RULES §2b), so an edge to it is no
/// dependency of one engine crate on another.
const BUILD_ONLY: &[&str] = &["workspace-hack"];

/// The first-party crates each tiered crate may list directly (RULES §3).
///
/// `khora-agents` may take everything above it except `khora-tool-ui`: the
/// SDK depends on the agents, and games never compile Khora's brand. Apps and
/// the example game go through the SDK; the two tools also take the brand.
const CLAD_TIERS: &[(&str, &[&str])] = &[
    // floor
    ("khora-core", &[]),
    ("khora-macros", &[]),
    // on the floor
    ("khora-data", &["khora-core", "khora-macros"]),
    ("khora-control", &["khora-core", "khora-data"]),
    ("khora-script", &["khora-core", "khora-macros"]),
    ("khora-telemetry", &["khora-core"]),
    ("khora-infra", &["khora-core"]),
    ("khora-tool-ui", &["khora-core"]),
    // middle
    (
        "khora-io",
        &[
            "khora-core",
            "khora-data",
            "khora-script",
            "khora-telemetry",
        ],
    ),
    (
        "khora-lanes",
        &[
            "khora-core",
            "khora-macros",
            "khora-data",
            "khora-io",
            "khora-script",
        ],
    ),
    // strategists
    (
        "khora-agents",
        &[
            "khora-core",
            "khora-macros",
            "khora-data",
            "khora-control",
            "khora-script",
            "khora-telemetry",
            "khora-infra",
            "khora-io",
            "khora-lanes",
        ],
    ),
    // façade
    (
        "khora-sdk",
        &[
            "khora-agents",
            "khora-control",
            "khora-core",
            "khora-data",
            "khora-infra",
            "khora-io",
            "khora-lanes",
            "khora-telemetry",
        ],
    ),
    // apps
    ("khora-editor", &["khora-sdk", "khora-tool-ui"]),
    ("khora-hub", &["khora-sdk", "khora-tool-ui"]),
    ("khora-runtime", &["khora-sdk"]),
    ("sandbox", &["khora-sdk"]),
];

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// `khora-core` is the floor: it lists no first-party crate.
#[test]
fn khora_core_depends_on_no_workspace_crate() {
    let graph = CrateGraph::read(&repo_root());
    assert_eq!(
        graph.direct("khora-core"),
        BTreeSet::new(),
        "khora-core must depend on no workspace crate"
    );
}

/// `xtask` depends on no workspace crate: `cargo gate` builds it first, and
/// an engine crate among its dependencies would compile the whole engine
/// before the gate's first step — on every change to the engine.
#[test]
fn xtask_depends_on_no_workspace_crate() {
    let graph = CrateGraph::read(&repo_root());
    assert_eq!(
        graph.direct("xtask"),
        BTreeSet::new(),
        "xtask must depend on no workspace crate: what needs the engine runs as the engine's own binary"
    );
}

/// `khora-infra` is a floor tenant, not a top layer: it takes `khora-core`
/// and nothing else, so the concrete backends moved into it (the tracking
/// allocator, the mix bus) cannot drag a higher crate down with them.
#[test]
fn khora_infra_depends_only_on_khora_core() {
    let graph = CrateGraph::read(&repo_root());
    assert_eq!(graph.direct("khora-infra"), names(&["khora-core"]));
}

/// `khora-tool-ui` — the tools' brand and widgets, and the dock tree — takes
/// `khora-core` and nothing else.
#[test]
fn khora_tool_ui_depends_only_on_khora_core() {
    let graph = CrateGraph::read(&repo_root());
    assert_eq!(graph.direct("khora-tool-ui"), names(&["khora-core"]));
}

/// Games never inherit Khora's brand: nothing the SDK depends on, directly or
/// through another crate, is `khora-tool-ui`.
#[test]
fn khora_sdk_never_reaches_khora_tool_ui() {
    let graph = CrateGraph::read(&repo_root());
    let reach = graph.reach("khora-sdk");
    assert!(
        !reach.contains("khora-tool-ui"),
        "khora-sdk reaches khora-tool-ui through {}",
        graph.path("khora-sdk", "khora-tool-ui").join(" -> ")
    );
}

/// Lanes sit below the strategists and the backends: nothing `khora-lanes`
/// depends on, directly or through another crate, is `khora-infra`.
#[test]
fn khora_lanes_never_reaches_khora_infra() {
    let graph = CrateGraph::read(&repo_root());
    let reach = graph.reach("khora-lanes");
    assert!(
        !reach.contains("khora-infra"),
        "khora-lanes reaches khora-infra through {}",
        graph.path("khora-lanes", "khora-infra").join(" -> ")
    );
}

/// Every first-party crate lists only the crates its CLAD tier allows, and
/// every crate of the graph has a tier: a new crate is placed in RULES §3
/// and in `CLAD_TIERS` before it builds.
#[test]
fn every_crate_stays_in_its_clad_tier() {
    let graph = CrateGraph::read(&repo_root());
    let tiers: BTreeMap<&str, BTreeSet<String>> = CLAD_TIERS
        .iter()
        .map(|(name, allowed)| (*name, names(allowed)))
        .collect();

    let mut problems = Vec::new();
    for (name, deps) in &graph.edges {
        if UNTIERED.contains(&name.as_str()) {
            continue;
        }
        let Some(allowed) = tiers.get(name.as_str()) else {
            problems.push(format!("{name}: no CLAD tier (add it to RULES §3)"));
            continue;
        };
        let upward: Vec<&String> = deps.difference(allowed).collect();
        if !upward.is_empty() {
            problems.push(format!("{name}: depends on {upward:?} outside its tier"));
        }
    }
    assert!(
        problems.is_empty(),
        "dependency edges outside the CLAD tiers:\n  {}",
        problems.join("\n  ")
    );
}

/// The reader sees the workspace as it is. Without this, a reader returning
/// no edge at all would pass every "depends on nothing" check above.
#[test]
fn graph_reader_sees_the_known_edges() {
    let graph = CrateGraph::read(&repo_root());
    for member in [
        "khora-core",
        "khora-macros",
        "khora-infra",
        "khora-tool-ui",
        "khora-lanes",
        "khora-sdk",
        "khora-editor",
        "khora-hub",
    ] {
        assert!(
            graph.edges.contains_key(member),
            "{member} missing from the graph: {:?}",
            graph.edges.keys().collect::<Vec<_>>()
        );
    }
    assert!(graph.direct("khora-sdk").contains("khora-agents"));
    assert!(graph.direct("khora-data").contains("khora-macros"));
    assert!(graph.direct("khora-editor").contains("khora-tool-ui"));
    assert!(graph.reach("khora-editor").contains("khora-core"));
    // A dev-dependency is not an edge: the hub's tests compile a seeded
    // script, the shipped hub links nothing of the language.
    assert!(!graph.direct("khora-hub").contains("khora-script"));
}

/// The reader follows `workspace = true`, target-specific tables, build
/// dependencies and `package` renames, and ignores dev dependencies and
/// registry crates.
#[test]
fn graph_reader_follows_every_spelling_of_an_edge() {
    let dir = std::env::temp_dir().join(format!(
        "khora-crate-graph-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);
    let setup = || -> std::io::Result<()> {
        for member in ["a", "b", "c", "d", "e", "f"] {
            fs::create_dir_all(dir.join(member))?;
        }
        fs::write(
            dir.join("Cargo.toml"),
            "[workspace]\nmembers = [\"a\", \"b\", \"c\", \"d\"]\n\
             [workspace.dependencies]\nc = { path = \"c\" }\n",
        )?;
        fs::write(
            dir.join("a/Cargo.toml"),
            "[package]\nname = \"a\"\n\
             [dependencies]\nc = { workspace = true }\nserde = \"1\"\n\
             renamed = { path = \"../e\", package = \"e\" }\n\
             [target.'cfg(windows)'.dependencies]\nb = { path = \"../b\" }\n\
             [build-dependencies]\nf = { path = \"../f\" }\n\
             [dev-dependencies]\nd = { path = \"../d\" }\n",
        )?;
        for member in ["b", "c", "d", "e", "f"] {
            fs::write(
                dir.join(member).join("Cargo.toml"),
                format!("[package]\nname = \"{member}\"\n"),
            )?;
        }
        Ok(())
    };
    let setup = setup();
    let graph = setup.as_ref().ok().map(|()| CrateGraph::read(&dir));
    let _ = fs::remove_dir_all(&dir);
    setup.expect("create temp workspace");
    let graph = graph.expect("graph read");

    assert_eq!(graph.direct("a"), names(&["b", "c", "e", "f"]));
    assert!(
        graph.edges.contains_key("e"),
        "a path crate outside the members is part of the graph"
    );
}

// ---------------------------------------------------------------------------
// Graph
// ---------------------------------------------------------------------------

/// Package name → the first-party packages it lists in a shipped table.
struct CrateGraph {
    edges: BTreeMap<String, BTreeSet<String>>,
}

impl CrateGraph {
    /// Reads the graph of the workspace rooted at `root`.
    fn read(root: &Path) -> Self {
        let root_manifest = read_manifest(&root.join("Cargo.toml"));
        let workspace = root_manifest
            .get("workspace")
            .and_then(Value::as_table)
            .unwrap_or_else(|| panic!("{} has no [workspace]", root.display()));
        let inherited = workspace
            .get("dependencies")
            .and_then(Value::as_table)
            .cloned()
            .unwrap_or_default();

        let mut queue: VecDeque<PathBuf> = members(root, workspace).into();
        let mut seen: BTreeSet<PathBuf> = queue.iter().cloned().collect();
        let mut edges = BTreeMap::new();

        while let Some(dir) = queue.pop_front() {
            let manifest = read_manifest(&dir.join("Cargo.toml"));
            let name = package_name(&manifest, &dir);
            if BUILD_ONLY.contains(&name.as_str()) {
                continue;
            }
            let mut deps = BTreeSet::new();
            for table in shipped_tables(&manifest) {
                for (key, spec) in table {
                    let Some(target) = path_target(root, &dir, &inherited, key, spec) else {
                        continue;
                    };
                    let target_manifest = read_manifest(&target.join("Cargo.toml"));
                    let target_name = package_name(&target_manifest, &target);
                    if BUILD_ONLY.contains(&target_name.as_str()) {
                        continue;
                    }
                    deps.insert(target_name);
                    if seen.insert(target.clone()) {
                        queue.push_back(target);
                    }
                }
            }
            edges.insert(name, deps);
        }
        Self { edges }
    }

    /// The first-party crates `name` lists directly.
    fn direct(&self, name: &str) -> BTreeSet<String> {
        self.edges
            .get(name)
            .cloned()
            .unwrap_or_else(|| panic!("{name} is not in the crate graph"))
    }

    /// Every first-party crate `name` depends on, directly or not.
    fn reach(&self, name: &str) -> BTreeSet<String> {
        let mut reached = BTreeSet::new();
        let mut stack: Vec<String> = self.direct(name).into_iter().collect();
        while let Some(next) = stack.pop() {
            if reached.insert(next.clone()) {
                stack.extend(self.edges.get(&next).into_iter().flatten().cloned());
            }
        }
        reached
    }

    /// One dependency chain from `from` to `to`, for a failure message; empty
    /// when there is none.
    fn path(&self, from: &str, to: &str) -> Vec<String> {
        let mut parent: BTreeMap<String, String> = BTreeMap::new();
        let mut queue = VecDeque::from([from.to_owned()]);
        while let Some(current) = queue.pop_front() {
            if current == to {
                let mut chain = vec![current];
                while let Some(up) = parent.get(chain.last().map(String::as_str).unwrap_or("")) {
                    chain.push(up.clone());
                }
                chain.reverse();
                return chain;
            }
            for next in self.edges.get(&current).into_iter().flatten() {
                if next != from && !parent.contains_key(next) {
                    parent.insert(next.clone(), current.clone());
                    queue.push_back(next.clone());
                }
            }
        }
        Vec::new()
    }
}

/// **No workspace crate has an optional feature, and none is switched on from
/// outside.** Cargo identifies a crate by its feature set: `cargo gate` and
/// `cargo t` pass `--all-features`, `cargo run -p khora-editor` does not, so
/// one optional feature low in the graph builds the whole engine twice
/// (RULES §2b). Every dependency table is read — shipped, dev and per target.
#[test]
fn no_workspace_crate_has_an_optional_feature() {
    let root = repo_root();
    let workspace = read_manifest(&root.join("Cargo.toml"));
    let workspace = workspace
        .get("workspace")
        .and_then(Value::as_table)
        .unwrap_or_else(|| panic!("the root manifest has no [workspace]"));
    const ALL_TABLES: &[&str] = &[
        "dependencies",
        "dev-dependencies",
        "dev_dependencies",
        "build-dependencies",
        "build_dependencies",
    ];
    let mut read = 0;
    let mut problems = Vec::new();
    for dir in members(&root, workspace) {
        let manifest = read_manifest(&dir.join("Cargo.toml"));
        let name = package_name(&manifest, &dir);
        read += 1;
        if let Some(features) = manifest.get("features").and_then(Value::as_table) {
            if !features.is_empty() {
                let listed: Vec<&String> = features.keys().collect();
                problems.push(format!("{name}: declares features {listed:?}"));
            }
        }
        let mut tables: Vec<&Table> = ALL_TABLES
            .iter()
            .filter_map(|key| manifest.get(*key).and_then(Value::as_table))
            .collect();
        if let Some(targets) = manifest.get("target").and_then(Value::as_table) {
            for target in targets.values().filter_map(Value::as_table) {
                tables.extend(
                    ALL_TABLES
                        .iter()
                        .filter_map(|key| target.get(*key).and_then(Value::as_table)),
                );
            }
        }
        for table in tables {
            for (dep, spec) in table {
                let Some(spec) = spec.as_table() else {
                    continue;
                };
                if spec.contains_key("path") && spec.contains_key("features") {
                    problems.push(format!("{name}: switches on features of `{dep}`"));
                }
            }
        }
    }
    assert!(
        read > 10,
        "the reader found only {read} workspace manifests"
    );
    assert!(
        problems.is_empty(),
        "optional features in the workspace, each a second build of the engine:\n  {}",
        problems.join("\n  ")
    );
}

/// **No floor crate declares an async runtime.** The floor crates
/// (`khora-core`, `khora-data`, `khora-script`, `khora-control`) are built
/// into every test binary above them (RULES §2b), and concurrency goes
/// through the DCC, not an executor (RULES §5): none lists `tokio` in any
/// dependency table — shipped, dev, build, or per target — under its own
/// name or renamed with `package = "tokio"`.
#[test]
fn floor_crates_declare_no_async_runtime() {
    const FLOOR: &[&str] = &["khora-core", "khora-data", "khora-script", "khora-control"];
    const RUNTIME: &str = "tokio";
    let root = repo_root();
    let workspace = read_manifest(&root.join("Cargo.toml"));
    let workspace = workspace
        .get("workspace")
        .and_then(Value::as_table)
        .unwrap_or_else(|| panic!("the root manifest has no [workspace]"));

    let mut found = BTreeSet::new();
    let mut problems = Vec::new();
    for dir in members(&root, workspace) {
        let manifest = read_manifest(&dir.join("Cargo.toml"));
        let name = package_name(&manifest, &dir);
        if !FLOOR.contains(&name.as_str()) {
            continue;
        }
        found.insert(name.clone());
        for (table_name, table) in every_dependency_table(&manifest) {
            for (dep, spec) in table {
                let package = spec
                    .as_table()
                    .and_then(|spec| spec.get("package"))
                    .and_then(Value::as_str)
                    .unwrap_or(dep);
                if package == RUNTIME {
                    problems.push(format!("{name}: [{table_name}] declares `{dep}`"));
                }
            }
        }
    }
    assert_eq!(
        found,
        names(FLOOR),
        "the reader did not find every floor crate among the members"
    );
    assert!(
        problems.is_empty(),
        "an async runtime in a floor crate:\n  {}",
        problems.join("\n  ")
    );
}

/// **The SDK declares no async runtime.** The engine boots before a tool's
/// startup work, which goes by ticket through the `loading` family, so the
/// SDK runs nothing on an executor of its own (RULES §5): `khora-sdk` lists
/// `tokio` in no dependency table — shipped, dev, build, or per target —
/// under its own name or renamed with `package = "tokio"`.
#[test]
fn the_sdk_declares_no_async_runtime() {
    const SDK: &str = "khora-sdk";
    const RUNTIME: &str = "tokio";
    let root = repo_root();
    let workspace = read_manifest(&root.join("Cargo.toml"));
    let workspace = workspace
        .get("workspace")
        .and_then(Value::as_table)
        .unwrap_or_else(|| panic!("the root manifest has no [workspace]"));

    let mut found = false;
    let mut problems = Vec::new();
    for dir in members(&root, workspace) {
        let manifest = read_manifest(&dir.join("Cargo.toml"));
        if package_name(&manifest, &dir) != SDK {
            continue;
        }
        found = true;
        for (table_name, table) in every_dependency_table(&manifest) {
            for (dep, spec) in table {
                let package = spec
                    .as_table()
                    .and_then(|spec| spec.get("package"))
                    .and_then(Value::as_str)
                    .unwrap_or(dep);
                if package == RUNTIME {
                    problems.push(format!("{SDK}: [{table_name}] declares `{dep}`"));
                }
            }
        }
    }
    assert!(found, "the reader did not find {SDK} among the members");
    assert!(
        problems.is_empty(),
        "an async runtime in the SDK:\n  {}",
        problems.join("\n  ")
    );
}

/// Every dependency table of a manifest — shipped, dev and build, top-level
/// and per target — with the name it is written under, for a failure message.
fn every_dependency_table(manifest: &Table) -> Vec<(String, &Table)> {
    const ALL_TABLES: &[&str] = &[
        "dependencies",
        "dev-dependencies",
        "dev_dependencies",
        "build-dependencies",
        "build_dependencies",
    ];
    let mut tables: Vec<(String, &Table)> = ALL_TABLES
        .iter()
        .filter_map(|key| {
            manifest
                .get(*key)
                .and_then(Value::as_table)
                .map(|table| ((*key).to_owned(), table))
        })
        .collect();
    if let Some(targets) = manifest.get("target").and_then(Value::as_table) {
        for (cfg, target) in targets {
            let Some(target) = target.as_table() else {
                continue;
            };
            tables.extend(ALL_TABLES.iter().filter_map(|key| {
                target
                    .get(*key)
                    .and_then(Value::as_table)
                    .map(|table| (format!("target.{cfg}.{key}"), table))
            }));
        }
    }
    tables
}

/// **A target that opts out of the tests holds none.** A target with
/// `test = false` is not built as a test — each such build would link the
/// whole engine for nothing — so a `#[test]` or `#[cfg(test)]` written in its
/// sources would never run. Every source file under the directory of such a
/// target's root file is read, its modules included.
#[test]
fn a_target_that_opts_out_of_the_tests_holds_none() {
    let root = repo_root();
    let workspace = read_manifest(&root.join("Cargo.toml"));
    let workspace = workspace
        .get("workspace")
        .and_then(Value::as_table)
        .unwrap_or_else(|| panic!("the root manifest has no [workspace]"));
    let mut opted_out = 0;
    let mut problems = Vec::new();
    for dir in members(&root, workspace) {
        let manifest = read_manifest(&dir.join("Cargo.toml"));
        for (kind, default) in [("lib", "src/lib.rs"), ("bin", "src/main.rs")] {
            let targets: Vec<&Table> = match manifest.get(kind) {
                Some(Value::Table(table)) => vec![table],
                Some(Value::Array(array)) => array.iter().filter_map(Value::as_table).collect(),
                _ => Vec::new(),
            };
            for target in targets {
                if target.get("test").and_then(Value::as_bool) != Some(false) {
                    continue;
                }
                opted_out += 1;
                let file = target
                    .get("path")
                    .and_then(Value::as_str)
                    .unwrap_or(default);
                let sources = dir.join(file);
                let sources = sources.parent().unwrap_or(&dir);
                for path in rust_files(sources) {
                    let text = fs::read_to_string(&path).unwrap_or_default();
                    if text.contains("#[test]") || text.contains("#[cfg(test)]") {
                        problems.push(path.display().to_string());
                    }
                }
            }
        }
    }
    assert!(
        opted_out > 0,
        "the reader found no target with `test = false`"
    );
    assert!(
        problems.is_empty(),
        "tests in a target marked `test = false`, which never run:\n  {}",
        problems.join("\n  ")
    );
}

/// **Only `LaneAgent` implements `Agent`** (`.agent/engine/RULES.md` §8): a
/// new subsystem is a family, whose agent is the one `LaneAgent`. Every `.rs`
/// file under `crates/` and `examples/` is read, its test code left out: a
/// file under a `tests/` directory, a file named `tests.rs`, a file marked
/// `#![cfg(test)]` or declared by its parent with `#[cfg(test)] mod …;` (and
/// the folder of such a module), and every item carrying `#[cfg(test)]`
/// (an inline `#[cfg(test)] mod … { … }` above all). Comments and string
/// literals are not code. Exactly one `impl … Agent for …` is left, and it is
/// `LaneAgent`'s.
#[test]
fn only_lane_agent_implements_agent() {
    let root = repo_root();
    let sources: Vec<(PathBuf, String)> = ["crates", "examples"]
        .iter()
        .flat_map(|dir| rust_files(&root.join(dir)))
        .map(|path| {
            let text = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            (path, blank_comments_and_literals(&text))
        })
        .collect();
    assert!(
        sources.len() > 100,
        "the reader found only {} Rust files under crates/ and examples/",
        sources.len()
    );
    let test_roots: Vec<PathBuf> = sources
        .iter()
        .flat_map(|(path, code)| test_module_roots(path, code))
        .collect();

    let mut impls = Vec::new();
    for (path, code) in &sources {
        let in_test_root = test_roots
            .iter()
            .any(|test| path == test || path.starts_with(test));
        if is_test_file(path) || in_test_root || has_inner_cfg_test(code) {
            continue;
        }
        for self_type in agent_impls(&non_test_code(code)) {
            let file = path.strip_prefix(&root).unwrap_or(path);
            impls.push(format!("{} for {self_type}", file.display()));
        }
    }

    assert_eq!(
        impls.len(),
        1,
        "exactly one `impl Agent for` outside test code, LaneAgent's; found:\n  {}",
        impls.join("\n  ")
    );
    assert!(
        impls[0].ends_with(" for LaneAgent"),
        "the one `impl Agent for` outside test code is not LaneAgent's: {}",
        impls[0]
    );
}

/// The reader of [`only_lane_agent_implements_agent`] sees every spelling of
/// an `impl … Agent for …` in code, and none in test code, a comment or a
/// string literal.
#[test]
fn the_agent_impl_reader_sees_every_spelling_outside_test_code() {
    let source = r###"
// impl Agent for InAComment {}
/// impl Agent for InADocComment {}
/* impl Agent for InABlockComment {} */
const TEXT: &str = "impl Agent for InAString {}";
const RAW: &str = r##"impl Agent for InARawString {}"##;
impl Agent for Plain {
    fn id(&self) -> char { '{' }
}
impl khora_core::agent::Agent for Qualified {}
impl<T: Fn() -> u32> Agent for Generic<T> where T: Send {}
impl AgentProvider for NotAnAgent {}
impl LaneAgent {}
fn returns() -> impl Agent { todo!() }
#[cfg(test)]
impl Agent for TestOnlyImpl {}
#[cfg(test)]
#[allow(dead_code)]
mod tests {
    fn brace() -> char { '}' }
    impl Agent for InATestModule {}
}
#[cfg(all(test, feature = "x"))] mod more { impl Agent for InAnotherTestModule {} }
impl Agent for AfterTheTests {}
"###;
    let found = agent_impls(&non_test_code(&blank_comments_and_literals(source)));
    assert_eq!(
        found,
        vec!["Plain", "Qualified", "Generic<T>", "AfterTheTests"],
        "the impls the reader saw"
    );

    let parent = Path::new("crates/a/src/lib.rs");
    let roots = test_module_roots(
        parent,
        &blank_comments_and_literals("#[cfg(test)]\nmod checks;\nmod shipped;\n"),
    );
    assert_eq!(
        roots,
        vec![
            PathBuf::from("crates/a/src/checks.rs"),
            PathBuf::from("crates/a/src/checks"),
        ],
        "a module declared under `#[cfg(test)]` is test code, its folder included"
    );
}

/// The reader of [`only_lane_agent_implements_agent`] sees an `impl` of
/// `Agent` whatever name the trait is imported under, and one written by a
/// `macro_rules!` that takes the trait as a parameter.
#[test]
fn the_agent_impl_reader_sees_the_trait_under_another_name() {
    let source = r###"
use khora_core::agent::Agent as Strategist;
impl Strategist for Renamed {}
macro_rules! strategist { ($tr:path, $t:ident) => { impl $tr for $t {} }; }
strategist!(khora_core::agent::Agent, ByMacro);
"###;
    let found = agent_impls(&non_test_code(&blank_comments_and_literals(source)));
    assert!(
        found.iter().any(|ty| ty == "Renamed"),
        "the impl of `Agent` imported as `Strategist`; the reader saw {found:?}"
    );
    assert!(
        found.iter().any(|ty| ty == "$t" || ty == "ByMacro"),
        "the impl a macro writes from its `$tr`; the reader saw {found:?}"
    );
}

/// Whether `path` holds only test code by its place: a `tests.rs`, or a file
/// under a `tests/` directory.
fn is_test_file(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == "tests.rs")
        || path
            .parent()
            .is_some_and(|dir| dir.components().any(|part| part.as_os_str() == "tests"))
}

/// Whether `code` carries the inner attribute `#![cfg(test)]` (or
/// `#![cfg(all(test, …))]`): the whole file is test code.
fn has_inner_cfg_test(code: &str) -> bool {
    code.lines().any(|line| {
        let compact: String = line.chars().filter(|ch| !ch.is_whitespace()).collect();
        compact == "#![cfg(test)]" || compact.starts_with("#![cfg(all(test,")
    })
}

/// `#[cfg(test)]` or `#[cfg(all(test, …))]`, never `not(test)`.
fn is_cfg_test(attribute: &str) -> bool {
    let compact: String = attribute.chars().filter(|ch| !ch.is_whitespace()).collect();
    compact == "#[cfg(test)]" || compact.starts_with("#[cfg(all(test,")
}

/// Whether `ch` can be part of an identifier.
fn is_ident(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

/// `text` with every comment and every string, raw string and character
/// literal replaced by spaces, line breaks kept: what is left is code only.
fn blank_comments_and_literals(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let at = |i: usize| chars.get(i).copied().unwrap_or('\0');
    let mut out: Vec<char> = Vec::with_capacity(n);
    let blank = |out: &mut Vec<char>, from: usize, to: usize| {
        for &ch in &chars[from..to.min(n)] {
            out.push(if ch == '\n' { '\n' } else { ' ' });
        }
    };
    let mut i = 0;
    while i < n {
        let ch = chars[i];
        let previous = if i > 0 { chars[i - 1] } else { '\0' };
        // A line comment, to the end of its line.
        if ch == '/' && at(i + 1) == '/' {
            let mut end = i;
            while end < n && chars[end] != '\n' {
                end += 1;
            }
            blank(&mut out, i, end);
            i = end;
            continue;
        }
        // A block comment, nested ones included.
        if ch == '/' && at(i + 1) == '*' {
            let (mut end, mut depth) = (i + 2, 1);
            while end < n && depth > 0 {
                if chars[end] == '/' && at(end + 1) == '*' {
                    depth += 1;
                    end += 2;
                } else if chars[end] == '*' && at(end + 1) == '/' {
                    depth -= 1;
                    end += 2;
                } else {
                    end += 1;
                }
            }
            blank(&mut out, i, end);
            i = end;
            continue;
        }
        // A raw string: `r"…"`, `r#"…"#`, `br"…"`.
        let starts_raw = ch == 'r'
            && (!is_ident(previous) || (previous == 'b' && (i < 2 || !is_ident(chars[i - 2]))))
            && {
                let mut hashes = i + 1;
                while at(hashes) == '#' {
                    hashes += 1;
                }
                at(hashes) == '"'
            };
        if starts_raw {
            let mut open = i + 1;
            while at(open) == '#' {
                open += 1;
            }
            let hashes = open - (i + 1);
            let mut end = open + 1;
            while end < n {
                if chars[end] == '"' && (1..=hashes).all(|k| at(end + k) == '#') {
                    end += 1 + hashes;
                    break;
                }
                end += 1;
            }
            blank(&mut out, i, end);
            i = end;
            continue;
        }
        // A string, its escapes included.
        if ch == '"' {
            let mut end = i + 1;
            while end < n && chars[end] != '"' {
                end += if chars[end] == '\\' { 2 } else { 1 };
            }
            blank(&mut out, i, end + 1);
            i = end + 1;
            continue;
        }
        // A character literal; a lifetime is code.
        if ch == '\'' {
            if at(i + 1) == '\\' {
                let mut end = i + 3;
                while end < n && chars[end] != '\'' {
                    end += 1;
                }
                blank(&mut out, i, end + 1);
                i = end + 1;
                continue;
            }
            if at(i + 2) == '\'' {
                blank(&mut out, i, i + 3);
                i += 3;
                continue;
            }
        }
        out.push(ch);
        i += 1;
    }
    out.into_iter().collect()
}

/// `code` (comments and literals blanked) without the items carrying
/// `#[cfg(test)]`: each such item is blanked through its balanced braces, or
/// through its `;` when it has none.
fn non_test_code(code: &str) -> String {
    let chars: Vec<char> = code.chars().collect();
    let n = chars.len();
    let mut out: Vec<char> = Vec::with_capacity(n);
    let mut pending = false;
    let mut i = 0;
    while i < n {
        let ch = chars[i];
        if ch.is_whitespace() {
            out.push(ch);
            i += 1;
            continue;
        }
        // An outer attribute, through its matching `]`.
        if ch == '#' && chars.get(i + 1) == Some(&'[') {
            let (mut end, mut depth) = (i + 1, 0);
            while end < n {
                match chars[end] {
                    '[' => depth += 1,
                    ']' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                end += 1;
            }
            let attribute: String = chars[i..(end + 1).min(n)].iter().collect();
            pending |= is_cfg_test(&attribute);
            out.extend(attribute.chars());
            i = end + 1;
            continue;
        }
        if pending {
            let (mut depth, mut opened) = (0i64, false);
            while i < n {
                let item = chars[i];
                out.push(if item == '\n' { '\n' } else { ' ' });
                i += 1;
                match item {
                    '{' => {
                        depth += 1;
                        opened = true;
                    }
                    '}' => {
                        depth -= 1;
                        if opened && depth == 0 {
                            break;
                        }
                    }
                    ';' if !opened && depth == 0 => break,
                    _ => {}
                }
            }
            pending = false;
            continue;
        }
        out.push(ch);
        i += 1;
    }
    out.into_iter().collect()
}

/// The modules `path` declares under `#[cfg(test)]` as files (`#[cfg(test)]
/// mod name;`): for each, its file `name.rs` and its folder `name/`, beside
/// `path` when it is a `lib.rs`, `main.rs` or `mod.rs`, in the folder named
/// after it otherwise.
fn test_module_roots(path: &Path, code: &str) -> Vec<PathBuf> {
    let Some(parent) = path.parent() else {
        return Vec::new();
    };
    let owns_its_folder = path
        .file_name()
        .is_some_and(|name| name == "lib.rs" || name == "main.rs" || name == "mod.rs");
    let folder = if owns_its_folder {
        parent.to_path_buf()
    } else {
        parent.join(path.file_stem().unwrap_or_default())
    };
    let mut roots = Vec::new();
    let mut pending = false;
    for line in code.lines() {
        let mut rest = line.trim();
        while rest.starts_with("#[") {
            let Some(close) = rest.find(']') else {
                break;
            };
            pending |= is_cfg_test(&rest[..=close]);
            rest = rest[close + 1..].trim_start();
        }
        if rest.is_empty() {
            continue;
        }
        if pending {
            let item = rest
                .strip_prefix("pub(crate) ")
                .or_else(|| rest.strip_prefix("pub(super) "))
                .or_else(|| rest.strip_prefix("pub "))
                .unwrap_or(rest);
            if let Some(declared) = item.strip_prefix("mod ") {
                let name: String = declared
                    .trim_start()
                    .chars()
                    .take_while(|ch| is_ident(*ch))
                    .collect();
                let after = declared.trim_start()[name.len()..].trim_start();
                if !name.is_empty() && after.starts_with(';') {
                    roots.push(folder.join(format!("{name}.rs")));
                    roots.push(folder.join(&name));
                }
            }
        }
        pending = false;
    }
    roots
}

/// The self type of every `impl … Agent for …` in `code` (comments and
/// literals blanked), its whitespace collapsed: the trait's last path
/// segment, its generic arguments aside, is `Agent` or a name `code` imports
/// it under (`Agent as X`). A trait that is a `macro_rules!` parameter
/// (`impl $tr for …`) counts too: the macro could be handed `Agent`.
fn agent_impls(code: &str) -> Vec<String> {
    let names = agent_names(code);
    let chars: Vec<char> = code.chars().collect();
    let n = chars.len();
    let at = |i: usize| chars.get(i).copied().unwrap_or('\0');
    let mut found = Vec::new();
    let mut i = 0;
    while i + 4 <= n {
        let is_impl = chars[i..i + 4] == ['i', 'm', 'p', 'l']
            && (i == 0 || !is_ident(chars[i - 1]))
            && !is_ident(at(i + 4));
        if !is_impl {
            i += 1;
            continue;
        }
        let mut j = i + 4;
        while j < n && chars[j].is_whitespace() {
            j += 1;
        }
        // The impl's own generics, `->` inside a bound aside.
        if at(j) == '<' {
            let mut depth = 0;
            while j < n {
                match chars[j] {
                    '<' => depth += 1,
                    '>' if j > 0 && chars[j - 1] != '-' => {
                        depth -= 1;
                        if depth == 0 {
                            j += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
        }
        // The header, up to the body or the end of the item.
        let mut end = j;
        while end < n && chars[end] != '{' && chars[end] != ';' {
            end += 1;
        }
        let header: Vec<char> = chars[j..end].to_vec();
        if let Some(self_type) = agent_impl_self_type(&header, &names) {
            found.push(self_type);
        }
        i = end.max(i + 4);
    }
    found
}

/// `Agent`, and every name `code` imports it under: each `Agent as X`, `X`
/// not `_`.
fn agent_names(code: &str) -> Vec<String> {
    let mut names = vec!["Agent".to_owned()];
    let words: Vec<&str> = code
        .split(|ch: char| !is_ident(ch))
        .filter(|word| !word.is_empty())
        .collect();
    for window in words.windows(3) {
        if window[0] == "Agent" && window[1] == "as" && window[2] != "_" {
            names.push(window[2].to_owned());
        }
    }
    names
}

/// The self type of the impl header `header` (what follows `impl` and its
/// generics), when the trait it implements is one of `names`, or a macro
/// parameter.
fn agent_impl_self_type(header: &[char], names: &[String]) -> Option<String> {
    let word_at = |i: usize, word: &str| {
        let len = word.chars().count();
        i + len <= header.len()
            && header[i..i + len].iter().copied().eq(word.chars())
            && (i == 0 || !is_ident(header[i - 1]))
            && header.get(i + len).is_none_or(|ch| !is_ident(*ch))
    };
    let mut depth = 0i32;
    let mut split = None;
    let mut where_at = header.len();
    for i in 0..header.len() {
        match header[i] {
            '<' | '(' => depth += 1,
            '>' if i > 0 && header[i - 1] == '-' => {}
            '>' | ')' => depth -= 1,
            _ if depth == 0 && split.is_none() && word_at(i, "for") => split = Some(i),
            _ if depth == 0 && split.is_some() && word_at(i, "where") => {
                where_at = i;
                break;
            }
            _ => {}
        }
    }
    let split = split?;
    let trait_path: String = header[..split].iter().collect();
    let trait_path = trait_path.trim().trim_start_matches('!');
    let trait_path = trait_path.split('<').next().unwrap_or("").trim();
    let last = trait_path.rsplit("::").next().unwrap_or("").trim();
    if !trait_path.starts_with('$') && !names.iter().any(|name| name == last) {
        return None;
    }
    let self_type: String = header[split + 3..where_at].iter().collect();
    Some(self_type.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// Every `.rs` file under `dir`, its subdirectories included.
fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return files;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            files.extend(rust_files(&path));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
    files
}

/// The member directories `workspace.members` names; `dir/*` expands to every
/// child directory holding a `Cargo.toml`.
fn members(root: &Path, workspace: &Table) -> Vec<PathBuf> {
    let listed = workspace
        .get("members")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("{} lists no workspace.members", root.display()));
    let mut dirs = Vec::new();
    for member in listed.iter().filter_map(Value::as_str) {
        if let Some(parent) = member.strip_suffix("/*") {
            let Ok(entries) = fs::read_dir(root.join(parent)) else {
                continue;
            };
            let mut children: Vec<PathBuf> = entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| path.join("Cargo.toml").is_file())
                .map(|path| normalize(&path))
                .collect();
            children.sort();
            dirs.extend(children);
        } else {
            dirs.push(normalize(&root.join(member)));
        }
    }
    dirs
}

/// The shipped dependency tables of a manifest: top-level and per target.
fn shipped_tables(manifest: &Table) -> Vec<&Table> {
    let mut tables: Vec<&Table> = SHIPPED_TABLES
        .iter()
        .filter_map(|key| manifest.get(*key).and_then(Value::as_table))
        .collect();
    if let Some(targets) = manifest.get("target").and_then(Value::as_table) {
        for target in targets.values().filter_map(Value::as_table) {
            tables.extend(
                SHIPPED_TABLES
                    .iter()
                    .filter_map(|key| target.get(*key).and_then(Value::as_table)),
            );
        }
    }
    tables
}

/// The directory a dependency entry points at, when it is a path dependency,
/// written in place or inherited from `[workspace.dependencies]`.
fn path_target(
    root: &Path,
    dir: &Path,
    inherited: &Table,
    key: &str,
    spec: &Value,
) -> Option<PathBuf> {
    let spec = spec.as_table()?;
    if let Some(path) = spec.get("path").and_then(Value::as_str) {
        return Some(normalize(&dir.join(path)));
    }
    if spec.get("workspace").and_then(Value::as_bool) == Some(true) {
        let path = inherited.get(key)?.as_table()?.get("path")?.as_str()?;
        return Some(normalize(&root.join(path)));
    }
    None
}

fn package_name(manifest: &Table, dir: &Path) -> String {
    manifest
        .get("package")
        .and_then(Value::as_table)
        .and_then(|package| package.get("name"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| panic!("{} has no package.name", dir.display()))
}

fn read_manifest(path: &Path) -> Table {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    toml::from_str(&text).unwrap_or_else(|error| panic!("cannot parse {}: {error}", path.display()))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| panic!("{} has no parent", env!("CARGO_MANIFEST_DIR")))
}

fn names(list: &[&str]) -> BTreeSet<String> {
    list.iter().map(|name| (*name).to_owned()).collect()
}

/// Resolves `.` and `..` lexically, so two spellings of one directory are one
/// key.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

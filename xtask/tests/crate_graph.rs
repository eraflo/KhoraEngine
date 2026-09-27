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
        &["khora-core", "khora-data", "khora-io", "khora-script"],
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
            let mut deps = BTreeSet::new();
            for table in shipped_tables(&manifest) {
                for (key, spec) in table {
                    let Some(target) = path_target(root, &dir, &inherited, key, spec) else {
                        continue;
                    };
                    let target_manifest = read_manifest(&target.join("Cargo.toml"));
                    deps.insert(package_name(&target_manifest, &target));
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

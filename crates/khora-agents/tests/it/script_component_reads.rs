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

//! A script reads the engine's components, end to end.
//!
//! Compiled against the mirrors the engine really serves, delivered the way
//! the engine delivers a program — through the reload channel — and run frame
//! after frame over a real `World`: the flow projects what the loaded programs
//! read, the agent runs the lane, and the `Maintenance` phase applies what the
//! scripts queued. A behavior writes what it read into its own fields, which
//! the test reads back from the scripting world.

use std::any::TypeId;
use std::sync::{Arc, Mutex};

use khora_agents::script_agent::ScriptAgent;
use khora_control::substrate::run_data_systems;
use khora_core::agent::gorna::{ResourceBudget, StrategyId};
use khora_core::agent::Agent;
use khora_core::ecs::entity::EntityId;
use khora_core::event::Channel;
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::script::{
    engine_event_channel, ComponentName, ComponentProjection, ScriptCatalog, ScriptValue,
};
use khora_core::{EngineContext, Runtime, WorldAccess};
use khora_data::ecs::{
    Collider, ComponentProvenance, FieldKind, PackedField, PackedLayout, RigidBody,
    RuntimeComponentDecl, Script, SemanticDomain, TickPhase, Transform, World,
};
use khora_data::flow::{Flow, ScriptFlow};
use khora_io::script::mirror::PreludeLoader;
use khora_lanes::script_lane::ScriptRuntime;
use khora_script::arena::Persisted;
use khora_script::reload::ScriptReload;
use khora_script::vm::{Program, Value};
use khora_script::{compile_module, Diagnostic, MemoryLoader, ENGINE_COMPONENTS_MODULE};

const MODULE: &str = "game/probe.erg";

/// `main` importing the engine's mirrors, compiled.
fn build(main: &str) -> Program {
    let source = format!("import \"engine/components.erg\";\n{main}");
    let loader = PreludeLoader::new(MemoryLoader::new().with("main.erg", source));
    let result = compile_module(&loader, "main.erg");
    let errors: Vec<&Diagnostic> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "should compile: {errors:?}");
    result.program.expect("a program with no errors")
}

fn body(mass: f32) -> RigidBody {
    RigidBody {
        mass,
        ccd_enabled: true,
        initial_velocity: khora_core::math::Vec3::new(1.0, 2.0, 3.0),
        ..RigidBody::default()
    }
}

/// One running session, wired the way the engine wires one: the catalog the
/// lane writes and the flow reads, the reload channel programs arrive by.
struct Session {
    world: World,
    flow: ScriptFlow,
    agent: ScriptAgent,
    scripts: Arc<Mutex<ScriptRuntime>>,
    reloads: Channel<ScriptReload>,
    catalog: Arc<ScriptCatalog>,
    runtime: Arc<Runtime>,
}

impl Session {
    /// A session over `world`, `program` sent as `MODULE` — the way the
    /// engine hands a compiled script to the lane.
    fn over(world: World, program: Program) -> Self {
        let scripts = Arc::new(Mutex::new(ScriptRuntime::new()));
        let reloads = khora_io::script::hot_reload::reload_channel();
        let catalog = Arc::new(ScriptCatalog::default());

        let mut runtime = Runtime::default();
        runtime.services.insert(Arc::clone(&scripts));
        runtime.resources.insert(reloads.clone());
        runtime.resources.insert(engine_event_channel());
        runtime.resources.insert(Arc::clone(&catalog));

        let mut agent = ScriptAgent::default();
        // The DCC does this every frame; without it there is no fuel.
        agent.apply_budget(ResourceBudget {
            strategy_id: StrategyId::Balanced,
            time_limit: std::time::Duration::from_millis(50),
            memory_limit: None,
            extra_params: Default::default(),
        });

        let session = Self {
            world,
            flow: ScriptFlow::default(),
            agent,
            scripts,
            reloads,
            catalog,
            runtime: Arc::new(runtime),
        };
        session.load(program);
        session
    }

    /// Sends `program` as the new `MODULE`, as a hot-reload would.
    fn load(&self, program: Program) {
        self.reloads.send(ScriptReload {
            module: MODULE.to_owned(),
            program,
        });
    }

    /// The first half of a frame: project, then run the scripts. Returns the
    /// deck holding what they queued, not yet applied, and what the flow
    /// projected.
    fn run_scripts(&mut self) -> (OutputDeck, Arc<ComponentProjection>) {
        let selection = self.flow.select(&self.world, &self.runtime);
        let mut view = self.flow.project(&self.world, &selection, &self.runtime);
        view.delta_seconds = 0.016;
        let projected = Arc::clone(&view.components);

        let mut bus = LaneBus::new();
        bus.publish(view);
        let mut deck = OutputDeck::new();
        let permit = self.agent.contention();
        {
            let mut ctx = EngineContext::for_agent(
                WorldAccess::None,
                Arc::clone(&self.runtime),
                &bus,
                &mut deck,
                &permit,
                Some(self.agent.id()),
            );
            self.agent.execute(&mut ctx);
        }
        (deck, projected)
    }

    /// The second half: the frame-boundary systems apply what was queued.
    fn apply(&mut self, mut deck: OutputDeck) {
        run_data_systems(
            &mut self.world,
            &self.runtime,
            &mut deck,
            TickPhase::Maintenance,
        );
    }

    /// One whole frame. Returns what the flow projected for it.
    fn frame(&mut self) -> Arc<ComponentProjection> {
        let (deck, projected) = self.run_scripts();
        self.apply(deck);
        projected
    }

    /// Field `name` of `entity`'s `behavior`, as the scripting world holds it
    /// — `None` while the installed program does not declare it yet (a
    /// reload that adds it is still waiting for its frame).
    fn field(&self, entity: EntityId, behavior: &str, name: &str) -> Option<Value> {
        let scripts = self.scripts.lock().expect("the scripting world");
        let slot = scripts.program(MODULE)?.layout(behavior)?.slot_of(name)?;
        match scripts.peek(entity, behavior)?.fields.get(slot)? {
            Persisted::Scalar(value) => Some(*value),
            _ => None,
        }
    }

    fn float(&self, entity: EntityId, behavior: &str, name: &str) -> Option<f32> {
        self.field(entity, behavior, name)?.as_float()
    }

    fn int(&self, entity: EntityId, behavior: &str, name: &str) -> Option<i64> {
        self.field(entity, behavior, name)?.as_int()
    }

    /// Runs frames until `ran` says the behavior has done its work, at most
    /// `limit`. Panics naming `what` if it never did.
    fn until(&mut self, limit: usize, what: &str, ran: impl Fn(&Self) -> bool) {
        for _ in 0..limit {
            self.frame();
            if ran(self) {
                return;
            }
        }
        panic!("after {limit} frames, {what}");
    }
}

/// Reads its own rigid body every frame: the mass, `-2` when it reads
/// `null`, and the first value it ever read.
const READS_ITS_OWN_BODY: &str = "behavior Probe {
    float mass = -1.0;
    float first = -1.0;
    void Update(float dt) {
        if (var b = this.Get(RigidBody)) { mass = b.mass; } else { mass = -2.0; }
        if (first == -1.0) { first = mass; }
    }
}";

// ─── Get and Has read the World ─────────────────────────────────────────────

/// **`this.Get(C)` is the entity's component, as the `World` holds it** — and
/// from its very first run: the program arrived needing `RigidBody`, so it
/// waits for the frame that projects it rather than reading `null`.
#[test]
fn get_reads_this_entitys_component() {
    let mut world = World::new();
    let probe = world.spawn((
        Transform::identity(),
        body(2.0),
        Script::new(MODULE, "Probe"),
    ));
    let mut session = Session::over(world, build(READS_ITS_OWN_BODY));

    session.until(4, "the probe never read its rigid body", |s| {
        s.float(probe, "Probe", "mass")
            .is_some_and(|mass| mass != -1.0)
    });

    assert_eq!(session.float(probe, "Probe", "mass"), Some(2.0));
    assert_eq!(
        session.float(probe, "Probe", "first"),
        Some(2.0),
        "the first read already found the component"
    );
}

/// **Any entity**: a component of the entity an `Entity` field names.
#[test]
fn get_reads_another_entitys_component() {
    let mut world = World::new();
    let target = world.spawn((Transform::identity(), body(7.5)));
    let watcher = world.spawn((
        Transform::identity(),
        Script::new(MODULE, "Watcher").with_field("target", ScriptValue::Entity(target)),
    ));
    let mut session = Session::over(
        world,
        build(
            "behavior Watcher {
                 Entity? target;
                 int aimed = 0;
                 float mass = -1.0;
                 void Update(float dt) {
                     if (var t = target) {
                         aimed = 1;
                         if (var b = t.Get(RigidBody)) { mass = b.mass; } else { mass = -2.0; }
                     }
                 }
             }",
        ),
    );

    session.until(4, "the watcher never read its target", |s| {
        s.float(watcher, "Watcher", "mass")
            .is_some_and(|mass| mass != -1.0)
    });

    assert_eq!(
        session.int(watcher, "Watcher", "aimed"),
        Some(1),
        "the target arrived"
    );
    assert_eq!(session.float(watcher, "Watcher", "mass"), Some(7.5));
}

/// An entity that does not hold the component reads `null` — while another
/// entity, holding it, is projected beside it.
#[test]
fn get_returns_null_when_absent() {
    let mut world = World::new();
    world.spawn((Transform::identity(), Collider::default()));
    let bare = world.spawn((Transform::identity(), Script::new(MODULE, "Probe")));
    let mut session = Session::over(
        world,
        build(
            "behavior Probe {
                 int seen = 0;
                 void Update(float dt) {
                     if (var c = this.Get(Collider)) { seen = 1; } else { seen = 2; }
                 }
             }",
        ),
    );

    session.until(4, "the probe never ran", |s| {
        s.int(bare, "Probe", "seen").is_some_and(|seen| seen != 0)
    });

    assert_eq!(session.int(bare, "Probe", "seen"), Some(2), "read null");
}

/// `Has` answers what the `World` holds: true for a holder, false otherwise.
#[test]
fn has_matches_the_world() {
    let mut world = World::new();
    let holder = world.spawn((
        Transform::identity(),
        body(1.0),
        Script::new(MODULE, "Probe"),
    ));
    let bare = world.spawn((Transform::identity(), Script::new(MODULE, "Probe")));
    let mut session = Session::over(
        world,
        build(
            "behavior Probe {
                 int has = 0;
                 void Update(float dt) { has = this.Has(RigidBody) ? 1 : 2; }
             }",
        ),
    );

    session.until(4, "the probes never ran", |s| {
        s.int(holder, "Probe", "has").is_some_and(|has| has != 0)
            && s.int(bare, "Probe", "has").is_some_and(|has| has != 0)
    });

    assert_eq!(session.int(holder, "Probe", "has"), Some(1));
    assert_eq!(session.int(bare, "Probe", "has"), Some(2));
}

// ─── The working set ────────────────────────────────────────────────────────

/// Reads its body, writes ten more, reads it back — within one `Update`.
const WRITES_THEN_READS: &str = "behavior Heavier {
    float before = -1.0;
    float later = -1.0;
    int ccd = 0;
    float lift = -1.0;
    void Update(float dt) {
        if (var b = this.Get(RigidBody)) {
            before = b.mass;
            this.Set(RigidBody { mass: b.mass + 10.0 });
            if (var c = this.Get(RigidBody)) {
                later = c.mass;
                ccd = c.ccd_enabled ? 1 : 2;
                lift = c.initial_velocity.y;
            }
        }
    }
}";

/// **A script sees its own write within the frame.** `Set` then `Get` in one
/// `Update` returns the value just written; the fields the `Set` did not name
/// are the `World`'s.
#[test]
fn a_script_sees_its_own_write_this_frame() {
    let mut world = World::new();
    let entity = world.spawn((
        Transform::identity(),
        body(2.0),
        Script::new(MODULE, "Heavier"),
    ));
    let mut session = Session::over(world, build(WRITES_THEN_READS));

    session.until(4, "the behavior never read its body", |s| {
        s.float(entity, "Heavier", "before")
            .is_some_and(|before| before != -1.0)
    });

    assert_eq!(
        session.float(entity, "Heavier", "before"),
        Some(2.0),
        "the World's"
    );
    assert_eq!(
        session.float(entity, "Heavier", "later"),
        Some(12.0),
        "its own write"
    );
    assert_eq!(
        session.int(entity, "Heavier", "ccd"),
        Some(1),
        "a field not written"
    );
    assert_eq!(
        session.float(entity, "Heavier", "lift"),
        Some(2.0),
        "a field not written, inside an engine value"
    );
}

/// **`Has` and `Get` follow `Add` and `Remove` within the frame**: an add
/// makes the component present, its value the defaults with the patch over
/// them; a remove makes it absent. The frame boundary then applies both.
#[test]
fn has_follows_add_and_remove_this_frame() {
    let mut world = World::new();
    let entity = world.spawn((
        Transform::identity(),
        body(2.0),
        Script::new(MODULE, "Swapper"),
    ));
    let mut session = Session::over(
        world,
        build(
            "behavior Swapper {
                 int done = 0;
                 int had = -1;
                 int added = -1;
                 float friction = -1.0;
                 float restitution = -1.0;
                 int removed = -1;
                 int gone = -1;
                 void Update(float dt) {
                     if (done == 1) { return; }
                     done = 1;
                     had = this.Has(Collider) ? 1 : 0;
                     this.Add(Collider { restitution: 0.25 });
                     added = this.Has(Collider) ? 1 : 0;
                     if (var c = this.Get(Collider)) { friction = c.friction; restitution = c.restitution; }
                     this.Remove(RigidBody);
                     removed = this.Has(RigidBody) ? 1 : 0;
                     if (var b = this.Get(RigidBody)) { gone = 0; } else { gone = 1; }
                 }
             }",
        ),
    );

    session.until(4, "the swapper never ran", |s| {
        s.int(entity, "Swapper", "done") == Some(1)
    });

    assert_eq!(session.int(entity, "Swapper", "had"), Some(0));
    assert_eq!(session.int(entity, "Swapper", "added"), Some(1), "added");
    assert_eq!(
        session.float(entity, "Swapper", "friction"),
        Some(Collider::default().friction),
        "a default"
    );
    assert_eq!(
        session.float(entity, "Swapper", "restitution"),
        Some(0.25),
        "the patch"
    );
    assert_eq!(
        session.int(entity, "Swapper", "removed"),
        Some(0),
        "removed"
    );
    assert_eq!(
        session.int(entity, "Swapper", "gone"),
        Some(1),
        "and reads null"
    );

    let world = &session.world;
    assert_eq!(
        world.get::<Collider>(entity).map(|c| c.restitution),
        Some(0.25),
        "the boundary attached the collider"
    );
    assert!(
        world.get::<RigidBody>(entity).is_none(),
        "and removed the body"
    );
}

/// **Engine systems see only the boundary.** While the script has already
/// read its own write, the `World` still holds the value from before the
/// frame; the `Maintenance` systems apply it, and only then does the `World`
/// change.
#[test]
fn engine_systems_see_only_the_boundary() {
    let mut world = World::new();
    let entity = world.spawn((
        Transform::identity(),
        body(2.0),
        Script::new(MODULE, "Heavier"),
    ));
    let mut session = Session::over(world, build(WRITES_THEN_READS));

    // Frames until the behavior has run once, its write held back.
    let mut held = None;
    for _ in 0..4 {
        let (deck, _) = session.run_scripts();
        if session
            .float(entity, "Heavier", "later")
            .is_some_and(|later| later != -1.0)
        {
            held = Some(deck);
            break;
        }
        session.apply(deck);
    }
    let deck = held.expect("the behavior ran within four frames");

    assert_eq!(
        session.float(entity, "Heavier", "later"),
        Some(12.0),
        "the script read its own write"
    );
    assert_eq!(
        session.world.get::<RigidBody>(entity).map(|b| b.mass),
        Some(2.0),
        "the World holds what it held before the frame"
    );

    session.apply(deck);

    assert_eq!(
        session.world.get::<RigidBody>(entity).map(|b| b.mass),
        Some(12.0),
        "the boundary applied the write"
    );
}

// ─── The read set ───────────────────────────────────────────────────────────

fn sorted(mut names: Vec<ComponentName>) -> Vec<ComponentName> {
    names.sort();
    names
}

fn names_of(list: &[&str]) -> Vec<ComponentName> {
    list.iter().map(|name| ComponentName::new(*name)).collect()
}

/// **Nothing reads, nothing is projected.** A program with no `Get` or `Has`
/// leaves the catalog empty and the view without a component.
#[test]
fn a_program_that_reads_nothing_projects_nothing() {
    let mut world = World::new();
    world.spawn((
        Transform::identity(),
        body(2.0),
        Collider::default(),
        Script::new(MODULE, "Mover"),
    ));
    let mut session = Session::over(
        world,
        build("behavior Mover { void Update(float dt) { this.Translate(Vec3(0.0, 1.0, 0.0)); } }"),
    );

    for _ in 0..3 {
        let projected = session.frame();
        assert!(projected.names().is_empty(), "nothing is projected");
    }
    assert!(
        session.catalog.reads().is_empty(),
        "the catalog reads nothing"
    );
}

/// **The read set follows reloads.** A program reading `RigidBody` projects
/// `RigidBody` alone; a reload adding a `Get(Collider)` projects `Collider`
/// from the next frame on — the frame the reload arrives in was projected
/// before it was applied.
#[test]
fn the_read_set_follows_reloads() {
    let mut world = World::new();
    world.spawn((
        Transform::identity(),
        body(2.0),
        Collider::default(),
        Script::new(MODULE, "Probe"),
    ));
    let mut session = Session::over(world, build(READS_ITS_OWN_BODY));

    for _ in 0..3 {
        session.frame();
    }
    assert_eq!(sorted(session.catalog.reads()), names_of(&["RigidBody"]));
    let before = session.frame();
    assert!(before.component("RigidBody").is_some());
    assert!(before.component("Collider").is_none());

    session.load(build(
        "behavior Probe {
             float mass = -1.0;
             float first = -1.0;
             float friction = -1.0;
             void Update(float dt) {
                 if (var b = this.Get(RigidBody)) { mass = b.mass; }
                 if (var c = this.Get(Collider)) { friction = c.friction; }
             }
         }",
    ));
    let arriving = session.frame();
    assert!(
        arriving.component("Collider").is_none(),
        "the frame the reload arrives in was projected before it"
    );
    assert_eq!(
        sorted(session.catalog.reads()),
        names_of(&["Collider", "RigidBody"]),
        "the reload's reads join the catalog"
    );
    let next = session.frame();
    assert!(
        next.component("Collider").is_some(),
        "the next frame projects them"
    );
    assert!(next.component("RigidBody").is_some());
}

/// **A program whose reads grew waits a frame**, so its first `Get` never
/// reads `null` for a component that exists: neither a program's first load
/// nor a reload that starts reading `Collider`.
#[test]
fn a_program_whose_reads_grew_waits_a_frame() {
    let mut world = World::new();
    let entity = world.spawn((
        Transform::identity(),
        body(2.0),
        Collider {
            friction: 0.9,
            ..Collider::default()
        },
        Script::new(MODULE, "Probe"),
    ));
    let grip = "behavior Probe {
        float mass = -1.0;
        float first = -1.0;
        int seen = 0;
        float friction = -1.0;
        void Update(float dt) {
            if (var c = this.Get(Collider)) {
                friction = c.friction;
                if (seen == 0) { seen = 1; }
            } else {
                seen = 2;
            }
        }
    }";

    // A first load that reads the collider at once.
    let mut first = Session::over(World::new(), build(grip));
    let arrived = first.world.spawn((
        Transform::identity(),
        Collider {
            friction: 0.9,
            ..Collider::default()
        },
        Script::new(MODULE, "Probe"),
    ));
    for _ in 0..4 {
        first.frame();
        assert_ne!(
            first.int(arrived, "Probe", "seen"),
            Some(2),
            "a first load read null"
        );
    }
    assert_eq!(first.int(arrived, "Probe", "seen"), Some(1), "and it read");

    // A reload that grows the reads of a program already running.
    let mut session = Session::over(world, build(READS_ITS_OWN_BODY));
    session.until(4, "the first version never ran", |s| {
        s.float(entity, "Probe", "mass")
            .is_some_and(|mass| mass != -1.0)
    });
    session.load(build(grip));
    for _ in 0..4 {
        session.frame();
        assert_ne!(
            session.int(entity, "Probe", "seen"),
            Some(2),
            "the reloaded program read null for a collider that exists"
        );
    }
    assert_eq!(session.int(entity, "Probe", "seen"), Some(1));
    assert_eq!(session.float(entity, "Probe", "friction"), Some(0.9));
}

/// **A program waiting a frame for its reads is not "unloaded".** It was
/// compiled and delivered; it waits, invisibly to the author, for the frame
/// that projects what it reads. Counting its instances as naming a module
/// nobody compiled raises the "is the script root mounted?" warning on every
/// start of a game whose scripts read a component.
#[test]
fn a_program_waiting_for_its_reads_is_not_reported_unloaded() {
    let mut world = World::new();
    world.spawn((
        Transform::identity(),
        body(2.0),
        Script::new(MODULE, "Probe"),
    ));
    let mut session = Session::over(world, build(READS_ITS_OWN_BODY));

    for frame in 0..3 {
        session.frame();
        let unloaded = session
            .scripts
            .lock()
            .expect("the scripting world")
            .last_report()
            .unloaded;
        assert_eq!(
            unloaded, 0,
            "frame {frame}: a compiled program waiting for its reads counted as unloaded"
        );
    }
}

/// **A waiting reload replaced before it installs never runs.** Two saves of
/// one module are one reload when they meet in the channel; one that meets the
/// other only after the first started waiting must not run the replaced code
/// for a frame either.
#[test]
fn a_waiting_reload_replaced_before_it_installs_never_runs() {
    let mut world = World::new();
    let entity = world.spawn((
        Transform::identity(),
        body(2.0),
        Collider::default(),
        Script::new(MODULE, "Probe"),
    ));
    let version = |n: i64, reads: &str| {
        build(&format!(
            "behavior Probe {{
                 int version = 0;
                 void Update(float dt) {{ {reads} version = {n}; }}
             }}"
        ))
    };
    let mut session = Session::over(world, version(1, ""));
    session.until(4, "the first version never ran", |s| {
        s.int(entity, "Probe", "version") == Some(1)
    });

    // Reads the rigid body: waits a frame.
    session.load(version(2, "var b = this.Get(RigidBody);"));
    session.frame();
    // Replaced while it waits, by one reading more: waits a frame too.
    session.load(version(
        3,
        "var b = this.Get(RigidBody); var c = this.Get(Collider);",
    ));
    for frame in 0..4 {
        session.frame();
        assert_ne!(
            session.int(entity, "Probe", "version"),
            Some(2),
            "frame {frame}: the replaced version ran"
        );
    }
    assert_eq!(session.int(entity, "Probe", "version"), Some(3));
}

/// **The read set shrinks with the programs.** A reload that no longer reads
/// a component stops its projection: nothing is projected that no loaded
/// program names.
#[test]
fn a_reload_that_stops_reading_stops_projecting() {
    let mut world = World::new();
    world.spawn((
        Transform::identity(),
        body(2.0),
        Script::new(MODULE, "Probe"),
    ));
    let mut session = Session::over(world, build(READS_ITS_OWN_BODY));
    for _ in 0..3 {
        session.frame();
    }
    assert!(session.frame().component("RigidBody").is_some());

    session.load(build(
        "behavior Probe { float mass = -1.0; void Update(float dt) { mass = 0.0; } }",
    ));
    session.frame();

    assert!(
        session.catalog.reads().is_empty(),
        "the catalog follows the reload"
    );
    assert!(
        session.frame().names().is_empty(),
        "and the next frame projects nothing"
    );
}

/// **One behavior reads what another wrote earlier in the same frame** — the
/// working set is the frame's, in the order behaviors run — while the `World`
/// holds the old value until the boundary.
#[test]
fn a_behavior_reads_what_an_earlier_one_wrote_this_frame() {
    let mut world = World::new();
    let target = world.spawn((Transform::identity(), body(2.0)));
    world.spawn((
        Transform::identity(),
        Script::new(MODULE, "Writer").with_field("target", ScriptValue::Entity(target)),
    ));
    let reader = world.spawn((
        Transform::identity(),
        Script::new(MODULE, "Reader").with_field("target", ScriptValue::Entity(target)),
    ));
    let mut session = Session::over(
        world,
        build(
            "behavior Writer {
                 Entity target;
                 void Update(float dt) { target.Set(RigidBody { mass: 9.0 }); }
             }
             behavior Reader {
                 Entity target;
                 float mass = -1.0;
                 void Update(float dt) {
                     if (var b = target.Get(RigidBody)) { mass = b.mass; }
                 }
             }",
        ),
    );

    let mut held = None;
    for _ in 0..4 {
        let (deck, _) = session.run_scripts();
        if session
            .float(reader, "Reader", "mass")
            .is_some_and(|mass| mass != -1.0)
        {
            held = Some(deck);
            break;
        }
        session.apply(deck);
    }
    assert!(held.is_some(), "the reader ran within four frames");
    assert_eq!(
        session.float(reader, "Reader", "mass"),
        Some(9.0),
        "the reader saw the writer's write"
    );
    assert_eq!(
        session.world.get::<RigidBody>(target).map(|b| b.mass),
        Some(2.0),
        "the World is unchanged until the boundary"
    );
}

/// **A component declared at run time is read by name.** The engine module
/// declares `Signal`, the `World` holds one, and a script reads it.
#[test]
fn get_of_a_declared_component() {
    let mut world = World::new();
    let signal = world
        .register_runtime_component(RuntimeComponentDecl {
            name: "Signal".to_owned(),
            domain: SemanticDomain::Spatial,
            provenance: ComponentProvenance::Authored,
            resumable: false,
            layout: PackedLayout::new(vec![PackedField {
                name: "strength".to_owned(),
                kind: FieldKind::Int,
                default: ScriptValue::Int(0),
            }])
            .expect("a valid layout"),
        })
        .expect("Signal registers");
    let beacon = world.spawn((Transform::identity(), Script::new(MODULE, "Receiver")));
    world
        .add_runtime_component(
            beacon,
            signal,
            &ScriptValue::Struct(vec![("strength".to_owned(), ScriptValue::Int(7))]),
        )
        .expect("attached");

    let loader = MemoryLoader::new()
        .with(ENGINE_COMPONENTS_MODULE, "component Signal { int strength; }")
        .with(
            "main.erg",
            format!(
                "import \"{ENGINE_COMPONENTS_MODULE}\";
                 behavior Receiver {{
                     int strength = -1;
                     void Update(float dt) {{
                         if (var s = this.Get(Signal)) {{ strength = s.strength; }} else {{ strength = -2; }}
                     }}
                 }}"
            ),
        );
    let result = compile_module(&loader, "main.erg");
    let errors: Vec<&Diagnostic> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "should compile: {errors:?}");
    let mut session = Session::over(world, result.program.expect("compiled"));

    session.until(4, "the receiver never ran", |s| {
        s.int(beacon, "Receiver", "strength")
            .is_some_and(|strength| strength != -1)
    });

    assert_eq!(session.int(beacon, "Receiver", "strength"), Some(7));
}

// ─── Declared and reported ──────────────────────────────────────────────────

/// **The agent declares the catalog it writes**, so a concurrent wave never
/// holds it beside the lane — and `EngineContext::locked` lets it through.
#[test]
fn the_agent_declares_the_catalog_it_writes() {
    let contention = ScriptAgent::default().contention();

    assert!(contention.may_lock(TypeId::of::<Arc<ScriptCatalog>>()));
}

/// **The cost of reads is visible.** The rows the frame projected reach the
/// lane's report and the agent's status.
#[test]
fn the_report_counts_the_projected_rows() {
    let mut world = World::new();
    for mass in [1.0, 2.0, 3.0] {
        world.spawn((Transform::identity(), body(mass)));
    }
    let probe = world.spawn((
        Transform::identity(),
        body(4.0),
        Script::new(MODULE, "Probe"),
    ));
    let mut session = Session::over(world, build(READS_ITS_OWN_BODY));

    session.until(4, "the probe never read its rigid body", |s| {
        s.float(probe, "Probe", "mass")
            .is_some_and(|mass| mass != -1.0)
    });
    let projected = session.frame();

    assert_eq!(projected.rows(), 4, "four holders of RigidBody");
    let rows = session
        .scripts
        .lock()
        .expect("the scripting world")
        .last_report()
        .projected_rows;
    assert_eq!(rows, 4);
    let status = session.agent.report_status().message;
    assert!(
        status.contains("4") && status.contains("projected"),
        "the status names the rows projected; got {status:?}"
    );
}

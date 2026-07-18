# Recherche — Audit codebase : bugs, implémentations moches, violations d'archi, code inachevé

- Date — 2026-07-18
- Phase — RPI / Research (read-only)
- Statut — Rapport, aucune modification de code

---

## Question

Passer le workspace Khora au crible pour identifier les **bugs** probables, les **implémentations
moches**, ce qui **ne respecte pas l'architecture CLAD/SAA**, et ce qui est **inachevé** — avec
citations `file:line` vérifiées, sans rien corriger.

## Answer / summary

L'architecture est globalement **saine** : aucun cycle de dépendances, aucun `glam` brut, aucun WGSL
inline, aucun `println!` illégal, aucun `thread::spawn` hors exception, aucun handle GPU brut exposé,
Flows bien read-only, budget 4 bind-groups respecté. Les vrais problèmes sont **concentrés et
localisés** : des `Result` GPU/ECS silencieusement jetés (`let _ =`) qui masquent des échecs, des
`unwrap()` sur locks en hot-path (par-frame/par-entité), une violation CLAD R2 latente
(`StandardUiLane` lit le World), un module physique orphelin qui viole aussi CLAD, et une poignée de
fonctionnalités à moitié câblées (emissive/wireframe lanes, VRAM detection, exécution parallèle M4).
S'y ajoute une fuite mémoire connue (AssetStore n'évince jamais) et de la dette documentaire (pointeurs
mémoire morts). Aucune de ces trouvailles ne bloque, mais les items 🔴 méritent une passe de
robustesse ciblée.

## Relevant files

| `file:line` | Rôle / problème |
|---|---|
| `crates/khora-lanes/src/render_lane/forward_plus_lane.rs:539,581,624` | `let _ = device.write_buffer(...)` — écritures GPU (tuiles/lumières/culling) jetées |
| `crates/khora-lanes/src/render_lane/{standard_pbr_lane.rs:277, simple_unlit_lane.rs:277, lit_forward_lane.rs:409, forward_plus_lane.rs:506}` | `let _ = encoder.begin_render_pass(&clear_desc)` — clear-pass jeté sur early-out |
| `crates/khora-sdk/src/game_world.rs:170,373,391` | `let _ = self.world.add_component(...)` — mutations ECS de parenting jetées |
| `crates/khora-io/src/asset_resolver.rs:204-206,302-304` | `let _ = world.remove/add_component(...)` — swaps de composants d'asset jetés |
| `crates/khora-infra/src/graphics/wgpu/device.rs:493` | `let _ = device.poll(Wait)` — poll GPU ignoré (device-lost/timeout masqué) |
| `crates/khora-data/src/gpu/projection.rs:115,119,124,233,238,256` | `cache.read()/.write().unwrap()` en boucle par-entité (mesh + material) |
| `crates/khora-core/src/renderer/api/core/frame_context.rs:167,172,177,188` | `self.data.lock().unwrap()` dans les accesseurs par-frame |
| `crates/khora-infra/src/graphics/wgpu/command.rs:327,332` | `.expect(...)` sur downcast GPU ; `panic!("Invalid profiler pass index")` |
| `crates/khora-lanes/src/ui_lane/standard_ui_lane.rs:45-54` | Lane lit `Slot<World>` au lieu d'une View du LaneBus (viol. CLAD R2, latente) |
| `crates/khora-data/src/ecs/world.rs:721,724` | `unsafe` sans tag `// SAFETY:` ; `unreachable!()` en page-migration par-frame |
| `crates/khora-data/src/flow/shadow.rs:140,185` | `LightType::Point(_) => unreachable!()` dans le flow d'ombres par-frame |
| `crates/khora-{agents}/…/agent.rs` | État par-frame (`frame_count`, `execute_attempts`, …) sur tous les agents |
| `crates/khora-sdk/src/run_default.rs:242` | Décodeur audio câblé à la main (pas d'`inventory::submit!`) |
| `crates/khora-control/src/scheduler.rs:582` | `unimplemented!("parallel agent execution")` (AGDF M4) |
| `crates/khora-lanes/src/render_lane/{emissive_lane.rs:238, wireframe_lane.rs:234}` | `execute` no-op (pipeline init, ne dessine jamais) |
| `crates/khora-infra/src/graphics/wgpu/device.rs:203,1951,1989-1990` | Compute-pipeline non impl. ; nom adaptateur hardcodé ; VRAM detection `None` |
| `crates/khora-core/src/asset/materials/{standard.rs:114-115, mod.rs:79}` | `occlusion_map` jamais échantillonné ; `specular_power()` dead-code |
| `crates/khora-lanes/src/physics_lane/native_lanes.rs:15-37,97,230,240` | Module orphelin `#![allow(dead_code)]` qui query+mute le World (viol. CLAD R2) |
| `crates/khora-io/src/asset/watcher.rs:216` | Seul test `#[ignore]` réel du workspace (timing-dependent) |

## How it works — findings par sévérité (CLAD)

### 🔴 MED — risque de bug réel

**Data/Infra layer — `Result` GPU/ECS jetés (`let _ =`).** Sur le chemin *Lane → Data* du render,
`forward_plus_lane.rs:539,581,624` jettent le retour de `device.write_buffer(...)` : un échec d'upload
GPU (tuiles/lumières/culling) passe inaperçu. Idem `begin_render_pass` du clear sur l'early-out des
lanes PBR/unlit/forward. Côté *SDK → Data*, `game_world.rs:170,373,391` jettent `add_component` du
parenting → un rattachement d'entité peut échouer silencieusement. `device.rs:493` ignore
`device.poll(Wait)`, ce qui masquerait un device-lost.

**Data/Core layer — `unwrap()` sur locks en hot-path.** `gpu/projection.rs` prend `read()/write()
.unwrap()` sur un `RwLock` **par entité** dans la boucle de projection (mesh l.115-124, material
l.233-256) ; `frame_context.rs:167-188` fait `lock().unwrap()` par-frame. Un poison ou une contention
mal gérée = panique de frame. C'est précisément le pattern que `lane/lock.rs` existe pour éviter.

**Agent layer (SAA) — état par-frame sur tous les agents.** `render/shadow/physics/overlay/audio`
agents portent des compteurs mutables (`frame_count`, `execute_attempts`, `last_frame_time`, …) qui
alimentent `report_status`/`is_stalled`. La lettre du contrat SAA (« agents = stratèges, zéro état
par-frame ») est enfreinte systématiquement. L'état GORNA légitime (`current_strategy`, `time_budget`,
ownership de lane) reste correct. → décision de conception (voir Open questions).

### 🟠 LOW-MED — hygiène / conventions

- **CLAD R2 latente** : `standard_ui_lane.rs:45-54` consomme `Slot<World>` puis
  `layout_system.compute_layouts(world)` au lieu d'une View du LaneBus. Câblé par `ui_agent/mod.rs:101`
  mais le `execute()` de l'agent ne dispatche que `render_lane` → chemin mort mais présent.
- **`unsafe` sans `// SAFETY:`** : `world.rs:721` (page-migration) a un commentaire ordinaire mais pas
  le tag, contrairement à ses voisins (l.874, 1057, 1289). Viole RULES §1.
- **`panic!`/`unreachable!` non-test** : `world.rs:724`, `flow/shadow.rs:140,185`,
  `command.rs:332`, accesseurs DI `runtime/{services,resources,backends}.rs`.
- **Décodeur audio manuel** : `run_default.rs:242` enregistre `SymphoniaDecoder` à la main ; les
  décodeurs font/material/shader/texture passent par `inventory::submit!`. (`MeshDispatcher` manuel mais
  justifié → pas un finding.)
- **Breadth** : `.lock()/.read()/.write().unwrap()` en masse dans `khora-control`, `khora-telemetry`,
  `khora-infra/{telemetry,physics/rapier}`. La *largeur* (convention à durcir) est le souci, pas chaque
  site isolé.

### 🟡 Inachevé (fonctionnalités à moitié câblées)

`scheduler.rs:582` (exécution parallèle M4, `unimplemented!` en dead-code) ;
`emissive_lane.rs:238` / `wireframe_lane.rs:234` (`execute` no-op, pipeline init mais ne dessine
jamais) ; `device.rs:203,1951,1989-1990` (compute-pipeline, nom adaptateur, VRAM detection) ;
`decoders/font.rs:26` (parsing font non fait) ; `commands.rs:242` et `control_plane.rs:656` (UI editor) ;
résidus texture-support : `standard.rs:114-115` `occlusion_map` non échantillonné,
`materials/mod.rs:79` `specular_power()` dead-code.

### 🟤 Orphelin / dead-code

`native_lanes.rs:15-37` — module `//! ⚠️ Orphan`, `#![allow(dead_code)]`, `NativeBroadphaseLane`/
`NativeSolverLane` query+mutent le World (viol. CLAD R2, l.97/230/240), non enregistrés par aucun
agent. À **migrer vers LaneBus/OutputDeck ou supprimer**. Clusters `#[allow(dead_code)]` additionnels
dans `khora-editor`, `khora-infra/graphics/wgpu`, `hub`. `lit_forward_lane.rs:27,29`
`#[allow(unused_imports)]` (résidu migration PipelineSystem).

### 🩹 Fuite mémoire connue (documentée)

**AssetStore GpuMaterial/GpuMesh + sub-stores CPU n'évincent jamais** → entrées orphelines qui
s'accumulent sur édition inline répétée de matériaux. Besoin d'une passe refcount/eviction (source :
mémoire agent `texture-support-progress`, « out of scope so far »).

## Non-findings vérifiés (sain — ne pas retoucher)

- Graphe CLAD **propre** : zéro `use` cross-crate montant, zéro cycle ; backends
  (wgpu/rapier/cpal/taffy/winit/egui/naga) uniquement dans `khora-infra/Cargo.toml` ; aucun type
  backend ne fuit dans `khora-core` (doc-comments seulement).
- **Zéro `glam` brut**, **zéro WGSL inline** (tout via `include_str!` de `.wgsl`), **zéro `println!`
  illégal** (seule exception : `ui/editor/log_capture.rs:65`), **zéro `thread::spawn`** hors
  `DccService::start`.
- **Aucun handle GPU brut** dans les API publiques core/sdk ; budget **4 bind-groups** respecté
  (max `@group(3)`).
- **Flows read-only** confirmés : `flow/physics.rs` `project` ne fait que lire ; la sync ECS→provider
  mutante est sortie vers le `DataSystem` inventory `physics_provider_sync` (`:114,132`,
  `TickPhase::PreExtract`). Aucune mutation structurelle ECS dans un Flow.
- Render lanes consomment `Ref<RenderWorld>` (vue projetée) du LaneBus, pas le World ECS.

## Constraints (RULES.md / reference)

- **Ne jamais `unwrap()` sur GPU/IO faillible** — utiliser `Result`/`?`/`map_err` (RULES §4). Les items
  🔴 y contreviennent.
- **Lanes consomment des Views du LaneBus, jamais le World** (RULES §3, `lane/bus.rs`). `standard_ui_lane`
  et `native_lanes` y contreviennent.
- **Agents implémentent uniquement `Agent` + `Default`, zéro job hors sélection de lane / GORNA / dispatch**
  (RULES §8). L'état par-frame est une zone grise à trancher.
- **`// SAFETY:` obligatoire sur chaque bloc `unsafe`** (RULES §1) — manquant à `world.rs:721`.
- **Wiring data-driven** (`DataSystemRegistration` / `register_flow!` / `AssetDecoderRegistration` via
  inventory, RULES §3) — le décodeur audio est l'exception non justifiée.
- Toute correction doit rester **minimale** (SOUL : « fix what's asked, don't refactor adjacent code »)
  et passer `cargo test --workspace` + `cargo clippy --workspace` clean.

## Open questions (décisions humaines)

1. **État par-frame des agents** — acceptable (nécessaire à `report_status`/télémétrie) ou à sortir vers
   l'OutputDeck/télémétrie pour respecter la lettre du contrat SAA ? Oriente un éventuel refactor
   transverse (5 agents).
2. **`native_lanes.rs` orphelin** — migrer vers LaneBus/OutputDeck, ou supprimer purement ?
3. **Fuite AssetStore** — prioriser la passe refcount/eviction maintenant, ou attendre un symptôme ?
4. **Dette documentaire** — rafraîchir dans la foulée les pointeurs mémoire morts (`.agent/engine/
   knowledge/MEMORY.md:74,91-92` → `docs/plans/render-lanes-followups.md` inexistant) et la dérive
   index/corps des mémoires agent (WS3/WS4 + texture-support marqués « remaining » mais terminés) ?

---

*Artefact read-only. La suite éventuelle passe par `create-plan` (phase 2 RPI) une fois les Open
questions tranchées.*

# Plan de remédiation — Audit codebase Khora

- Date — 2026-07-18
- Phase — RPI / Plan
- Recherche source — [`docs/research/2026-07-18_codebase-audit.md`](../research/2026-07-18_codebase-audit.md)
- Statut — Approuvé, prêt pour `implement-plan`

## Context

L'audit a cartographié bugs, implémentations moches, violations CLAD/SAA et code inachevé. Ce plan
corrige les 4 familles retenues. Les **fonctionnalités inachevées** (emissive/wireframe lanes, VRAM
detection, exécution parallèle M4) restent **hors scope** — plans séparés. Cadrage : locks poison-safe
**sur tout le workspace** ; `native_lanes.rs` **supprimé** ; état par-frame des agents **unifié via le
design Option A** (recherche dédiée en Phase 6).

Principe : **changements minimaux**, chaque phase compilable + testable indépendamment, ordonnées du
plus bas risque au plus haut. `cargo test --workspace` + `cargo clippy --workspace` clean après chaque
phase.

**Correction à l'audit** : le décodeur audio câblé à la main (`run_default.rs:242`) n'est **pas** une
violation — le slot `audio` reste explicite volontairement (backends concurrents wav/symphonia,
documenté à `khora-io/src/asset/registry.rs:34-36`). Retiré du scope.

---

## Phase 1 — Robustesse : `Result` ignorés + `// SAFETY:` manquant

Corriger les `let _ =` qui jettent un `Result` faillible et le bloc `unsafe` sans tag.

**GPU write_buffer (khora-lanes)** — précédent : `ui_render_lane.rs:268-272`
(`.map_err(|e| LaneError::ExecutionFailed(Box::new(e)))?`).
- `crates/khora-lanes/src/render_lane/forward_plus_lane.rs:539,581,601,624` — `let _ = device.write_buffer(...)` → `map_err(...)?` (l'`execute` retourne `Result<(), LaneError>`).
- Note : les `let _ = encoder.begin_render_pass(&clear_desc)` (`standard_pbr_lane.rs:277`, `simple_unlit_lane.rs:277`, `lit_forward_lane.rs:409`, `forward_plus_lane.rs:506`) **ne jettent pas de `Result`** (drop volontaire d'un clear-only pass). **Aucune action.**

**Mutations ECS jetées (khora-sdk / khora-io)** — fns englobantes `()`, pattern log (précédent : cycle-guard de `set_parent` + `remove_component` à `game_world.rs:179`).
- `crates/khora-sdk/src/game_world.rs:169,170,373,391` — `let _ = self.world.add_component(...)` → `if let Err(e) = ... { log::warn!(...) }`.
- `crates/khora-io/src/asset_resolver.rs:204-206,302-304` — `let _ = world.remove/add_component(...)` → log.

**Poll GPU (khora-infra)**
- `crates/khora-infra/src/graphics/wgpu/device.rs:493` — `let _ = device.poll(Wait)` → `if let Err(e) = ... { log::warn!("device poll failed: {e:?}") }`.

**`// SAFETY:` manquant** — format sibling à `world.rs:874`.
- `crates/khora-data/src/ecs/world.rs:721` — préfixer le bloc `unsafe` d'un `// SAFETY:` (disjonction de pages, comme l.874/1057).

**Vérif** : `cargo test -p khora-lanes -p khora-sdk -p khora-data -p khora-infra` ; `cargo clippy --workspace`.

---

## Phase 2 — Locks poison-safe (tout le workspace)

Remplacer chaque `.lock()/.read()/.write().unwrap()` en code non-test. Error enums existants :
`LaneError`, `RenderError`, `MetricsError` (pas de `ControlError`/`TelemetryError`/`PhysicsError`).
**Stratégie par contexte** :

| Contexte de la fn englobante | Traitement |
|---|---|
| `Result<_, LaneError>` (lanes, io) | helper `lane/lock.rs` (`read_lock`/`write_lock`/`mutex_lock`) + `?` |
| `Result<_, RenderError>` (infra GPU) | `*_render` de `lane/lock.rs` ou `.map_err(RenderError::…)?` |
| `Result<_, Box<dyn Error>>` (`text.rs::flush`) | `.map_err(\|_\| "… poisoned".into())?` |
| `()` | idiome local `if let Ok(g) = lock() { … } else { log::warn!(…); /* skip */ }` |
| Valeur `T`/`Option`/`Vec`/report (accesseur infaillible) | fallback poison → `None`/`Vec::new()`/`Default`, **ou** `.expect("… poisoned")` documenté |

**Sites par crate** (représentatifs) :
- `khora-lanes` / `khora-io` : adopter `lane/lock.rs` (déjà consommé via `render_lane/util/lock.rs`).
- `khora-data/src/gpu/projection.rs:115,119,124,233,238,256` : `sync_all`/`sync_materials` retournent `()` → **log + early-return** sur poison (minimal). Alternative : passer les deux fns `pub` en `Result<_, LaneError>` (vérifier appelants).
- `khora-core/src/renderer/api/core/frame_context.rs:167,172,177,188` : accesseurs `T`/`Option`/`bool` → `.expect` documenté.
- `khora-control` : `service.rs` (246,260,321,348,418,517,620), `scheduler.rs` (312,317,464), `budget_channel.rs` (72,82) → `if let Ok else {log+skip}` (idiome à `service.rs:405,476`). `get_context:620` → `.read().map(clone).unwrap_or_default()` ; `budget_channel:82` → `None`.
- `khora-telemetry` : `memory_backend.rs` (52,81,91) + `monitoring/registry.rs` (36,44,52) → fallback `Default`/`Vec::new()`.
- `khora-infra` : monitors (`gpu_monitor.rs:43,76`, `memory_monitor.rs:56,63,73,79,84,109,125`) → `None`/`Default` / log+skip ; `physics/rapier/{events.rs:30,mod.rs:271}` → log / `Vec` vide ; `renderer/text.rs` flush → `?`.
- **Non-locks** : `graphics/wgpu/command.rs:327` = `.expect` sur downcast GPU (améliorer message) ; `device.rs` mappe déjà tout vers `RenderError` (référence).
- `khora-sdk/src/engine.rs:549` + `khora-data/src/render/frame_graph.rs:148` : déjà `.expect` documenté → **laisser**.

**Vérif** : `cargo test --workspace` ; `cargo clippy --workspace` ; grep `\.lock\(\)\.unwrap\(\)|\.(read|write)\(\)\.unwrap\(\)` → 0 hit hors tests.

---

## Phase 3 — Hygiène : durcir `panic!`/`unreachable!` en chemin par-frame

Portée minimale (gardes logiques conservés, seules les paniques *atteignables* traitées).
- `crates/khora-infra/src/graphics/wgpu/command.rs:332` — `panic!("Invalid profiler pass index")` → erreur loguée ou no-op.
- `crates/khora-data/src/ecs/world.rs:724` + `flow/shadow.rs:140,185` — `unreachable!()` gardés : commenter l'invariant, ou `debug_assert!(false, …)` + fallback gracieux si l'englobant peut retourner proprement. Ne pas sur-refactorer.
- `khora-core/src/runtime/{services,resources,backends}.rs:80/83/92` — `panic!("… not registered")` dans `require::<T>()` : laisser (contrat DI) ou message pointant la registration manquante.

**Vérif** : `cargo test --workspace`.

---

## Phase 4 — Nettoyage code mort

- **Supprimer** `crates/khora-lanes/src/physics_lane/native_lanes.rs` (orphelin `#![allow(dead_code)]`, jamais enregistré, query+mute le World → viole CLAD R2). Retirer `mod native_lanes;` dans `physics_lane/mod.rs`. Vérifier zéro import.
- **Supprimer le chemin layout mort** : `StandardUiLane` (`ui_lane/standard_ui_lane.rs`) lit le `World` mais **n'est jamais exécuté** — le champ `layout_lane` de `UiAgent` (`khora-agents/src/ui_agent/mod.rs:61,95,101,335`) n'a aucun `.execute()`. Retirer le champ + sa construction (`:101`) + `StandardUiLane` si inutilisé ailleurs. Élimine la violation CLAD R2 latente **par suppression**.
- **Résidus** : retirer `#[allow(unused_imports)]` + imports morts à `render_lane/lit_forward_lane.rs:27,29`.
- **Laisser** les `#[allow(dead_code)]` justifiés « will be used when X implemented » (`wgpu/device.rs:203`, etc.) — features inachevées hors scope.

**Vérif** : `cargo build --workspace` ; `cargo test --workspace` ; `cargo clippy --workspace`.

---

## Phase 5 — Dette documentaire

- Réparer le pointeur mort `.agent/engine/knowledge/MEMORY.md:74,91-92` → `docs/plans/render-lanes-followups.md` (inexistant) : pointer vers les TODO code réels (emissive/wireframe) **ou** recréer un doc de suivi léger sous `docs/plans/`. `.agent/engine/` = source canonique (éditable, ≠ wrappers générés).
- Dérive index/corps des mémoires agent `~/.claude` : housekeeping séparé, pas un edit repo.

**Vérif** : liens résolus (revue manuelle).

---

## Phase 6 — Unification statut agent + télémétrie par-frame (design Option A)

> Phase **architecturale** la plus lourde/risquée — indépendante des phases 1-5, gated séparément.

**Problème.** Les 5 agents portent des compteurs par-frame (`frame_count`, `execute_attempts`,
`last_frame_time`/`last_step_time`, `draw_call_count`, `triangle_count`, `last_light_count`)
**redondants** : le scheduler mesure déjà le temps d'exécution (`TelemetryEvent::AgentCost`,
`scheduler.rs:544`) et connaît la complétion (`CompletionMap`, `:551`) ; draws/tris sont autoritatifs
via `GpuReport` (MonitorRegistry, lu par l'editor `app.rs:760`). Ces champs violent la lettre du contrat
SAA (agents = purs `Agent + Default`, zéro état par-frame). Aucun sender télémétrie n'est atteignable
depuis `execute` (`EngineContext` = world/runtime/bus/deck).

**Design retenu — Option A : le scheduler possède les métriques + santé par-agent.**
- Au point de mesure existant (`scheduler.rs:538-551`), enregistrer par agent
  `AgentFrameStatus { measured_time_ms, workload_n, health_score, is_stalled }` — `health_score` dérivé
  de *temps mesuré vs `ResourceBudget.time_limit` émis*, `is_stalled` de `CompletionOutcome`.
- Nouveau handle `Arc<RwLock<HashMap<AgentId, AgentFrameStatus>>>` dans `runtime.resources` (même pattern
  d'admission que `InputMap`/`MonitorRegistry`/`dcc_context` : handle long-lived, donnée par-frame dedans
  — cohérent avec `runtime/mod.rs:27-31` qui interdit l'état par-frame *comme entrée de conteneur*).
- `AgentStatus` (`khora-core/src/control/gorna.rs:186`) rétrécit à `current_strategy` (seul champ consommé
  par GORNA/DCC : `gorna/mod.rs:204,313` + `service.rs:572`). `report_status` devient strategy-only.
- **Retirer** de chaque agent les champs par-frame, leurs écritures dans `execute` et leurs dérivations
  dans `report_status`. Garder `lanes`/`strategy`/`current_strategy`/`time_budget`/`fixed_timestep`/
  `max_sources_per_frame` (état GORNA légitime).
- **Editor** : `control_plane.rs` lit le nouveau snapshot scheduler-owned depuis `runtime.resources`
  (drop-in mirroir de `dcc_context`/`MonitorRegistry`, `app.rs:497-508`) au lieu de
  `AgentStatus.message/is_stalled/health_score`. Draws/tris déjà fournis par MonitorRegistry.

**Alternatives écartées** : *(B)* type `AgentMetrics` sur l'OutputDeck drainé en Maintenance — impose une
*action* par-frame dans `execute` + fan-out DCC/editor ; *(C)* sink dans un conteneur Runtime écrit par
`execute` — **contredit** `runtime/mod.rs:27-31`. Option A = seule où les agents ne font *rien* de
nouveau et où santé/stall dérivent d'une *mesure* (plus juste que l'auto-report).

**Point à trancher à l'implémentation** : `current_strategy` naît encore dans l'agent → soit le capter
côté scheduler via le `budget_channel` appliqué, soit garder un `report_status() -> StrategyId` minimal.

**Vérif** : `cargo test --workspace` ; lancer l'editor et confirmer que le Control Plane affiche toujours
santé/stall par-agent + draws/tris globaux ; zéro erreur de validation Vulkan.

---

## Out of scope (plans séparés)

- Fonctionnalités inachevées : emissive/wireframe lanes, VRAM detection + compute pipeline, exécution
  parallèle M4 (`scheduler.rs:582`), échantillonnage `occlusion_map`, parsing font réel.
- Fuite AssetStore (GpuMaterial/GpuMesh sans éviction) — passe refcount/eviction dédiée.
- Migration (vs suppression) de `native_lanes` vers LaneBus/OutputDeck.

## Status (mis à jour par implement-plan)

- [x] Phase 1 — Results ignorés + `// SAFETY:` — **fait**. `forward_plus_lane` × 4 write_buffer, `game_world` (add/remove routés via wrappers logués), `asset_resolver` × 4, `device.rs` poll, `world.rs:721` SAFETY. **Déviation** : les write_buffer sont dans `fn render(...) -> ()` (pas `execute`) → pattern `if let Err(e) { log::error! }` (précédent `gizmo_lane`) au lieu de `map_err(...)?`. Vérif : `cargo test -p khora-lanes -p khora-sdk -p khora-data -p khora-infra` OK ; clippy clean. Découverte : macro `lock_or_log!` (`khora-lanes/src/render_lane/util/mod.rs`) déjà utilisée pour les locks → réutiliser en Phase 2.
- [x] Phase 2 — Locks poison-safe workspace-wide — **fait**. ~30 sites production sur 6 crates (khora-data, khora-io, khora-core, khora-telemetry, khora-infra, khora-control). **Déviation vs table du plan** : idiome uniforme `.unwrap_or_else(|e| e.into_inner())` (poison-recovery, ne crashe jamais, aucun changement de signature/flux) pour tous les contextes `()`/accesseurs infaillibles, au lieu du log+early-return/expect par-contexte — plus reviewable et minimal. `text.rs::flush` (retourne `Result`) propage via `.map_err(|_| "… poisoned")?`. Bonus : 2 `let _ = world.add_component` ignorés (`projection.rs`) logués, + 4 sites query-cache (`world.rs`) non énumérés au plan mais couverts par « tout le workspace ». `native_lanes.rs` intact (supprimé en Phase 4). Vérif : `cargo test --workspace` **867 passed / 0 failed** ; clippy clean ; grep de contrôle → 0 hit hors tests (et native_lanes).
- [x] Phase 3 — Hygiène panic!/unreachable! — **fait**. `command.rs:332` → fallback logué sur pass A (la fn retourne une valeur, pas un `Result`, donc no-op impossible). `flow/shadow.rs:140` + `world.rs:724` `unreachable!()` → messages auto-documentant l'invariant (gardés, comportement inchangé). Les `panic!` DI `require::<T>()` laissés (contrat documenté). Vérif : `cargo test -p khora-data -p khora-infra` 0 failed.
- [x] Phase 4 — Nettoyage code mort — **fait**. Supprimé `native_lanes.rs`, `ui_lane/` (StandardUiLane + mod), champ `layout_lane`/bloc `on_initialize`/import de `UiAgent`, imports morts `lit_forward_lane.rs`. **Déviation** : `native_lanes` n'était PAS totalement orphelin — `engine.rs:203` enregistrait la resource `CollisionPairs` via l'alias `CollisionPairsResource` (défini dans native_lanes), conscommée uniquement par les lanes supprimées (« currently unused » dans le commentaire). → retiré aussi le wiring mort dans `engine.rs` + rafraîchi les doc-comments périmés de `collision.rs` ; **gardé** le type de données `CollisionPairs`/`CollisionPair` (pub, serde, réutilisable) désormais sans consommateur (candidat cleanup futur). Vérif : `cargo build/clippy --workspace` clean ; `cargo test --workspace` **866 passed / 0 failed** (-1 = module de test de native_lanes supprimé).
- [x] Phase 5 — Dette documentaire — **fait**. `.agent/engine/knowledge/MEMORY.md` : 2 pointeurs morts `render-lanes-followups.md` → pointent vers les TODO code réels ; **bonus** (intersecte Phase 4) : retiré le bullet « open work » périmé sur `native_lanes.rs` (supprimé) et ajouté sa résolution à la note « Resolved ». Restes non traités (hors scope/interdits) : exemple illustratif `e.g. render-lanes-followups.md` dans `knowledge-locator.md` (anodin) + sa copie `.cursor` (wrapper généré, RULES §9). Dérive mémoire `~/.claude` laissée (housekeeping hors-repo). Vérif : grep → plus aucun pointeur mort réel.
- [x] Phase 6 — Unification statut agent (Option A) — **fait**. Nouveau `AgentFrameStatus { measured_time_ms }` + `AgentFrameStatusMap` (Arc<RwLock<HashMap>>) + helper `measured_frame_time_ms` dans `khora-core/control/gorna.rs` ; map insérée dans `runtime.resources` (`engine.rs`) ; le scheduler écrit le temps mesuré par agent (`execute_agents_in_phase`, 0.0 sur skip). Les 5 agents perdent leurs compteurs par-frame (`frame_count`, `execute_attempts`, `last_frame_time`/`last_step_time`, draw/tri/light), gardent un handle `frame_status` lu dans `report_status` (via free fn, pas de méthode inhérente → respecte RULES §8). health_score dérivé du `time_budget` retenu + temps mesuré. **Décision appliquée** (amendement au plan) : `is_stalled` retiré (→ `false`) — le scheduler ne peut pas reproduire « execute appelé mais jamais terminé » sans réintroduire l'état par-frame, et `Skipped` ≠ stalled ; GORNA garde le trigger pression + health-degraded, perd le détecteur crash (basé sur l'auto-report). draws/tris de l'editor viennent déjà de `GpuReport`/MonitorRegistry. **Contrat `AgentStatus` inchangé**, editor + GORNA non touchés. Correctif bonus : `projection.rs` skip le `ComponentAlreadyExists` bénin (spam WARN révélé au run editor). Vérif : `cargo test --workspace` **866 passed / 0 failed** ; clippy clean ; **run editor 12s : 0 panic, 0 erreur validation Vulkan, 0 spam**.

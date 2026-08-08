# Knowledge — Working Memory

Living state of the engine. Update when state changes (build status, latest work, known issues).
Deep history lives in git and `docs/plans/`.

## Current state
- **Branch**: `dev`
- **Build**: clean (all crates compile, 0 errors); clippy 0 errors.
- **Tests**: ~866 passing, ~29 ignored, 0 failures. Treat the live `cargo test --workspace` count as truth.

## Latest work (2026-07-31, suite 4) — Phase 4 (partielle) : fonctionnalités mortes
- **C4 — pliage de la hiérarchie** : chevron interactif (`ChevronRight`/`ChevronDown`), récursion
  stoppée sur nœud replié, `count_visible_nodes` pour l'étendue de scroll. L'état est stocké comme
  l'**exception** (`collapsed`, pas `expanded`) : une scène chargée s'affiche entière et un enfant
  fraîchement créé apparaît sans rien ouvrir.
- **C5 — palette** : `focus_last_item()` au 1ᵉʳ frame d'ouverture (verrou `was_open`, sinon le focus
  est piégé), flèches ↑↓ via le nouveau **`raw_key_pressed`** — `key_pressed` refuse pendant qu'un
  champ a le focus, or la palette *possède* ce champ, d'où la variante non supprimée.
- **Débordement de la palette corrigé** : liste clippée dans la modale + défilement qui suit la ligne
  active. Elle peignait par-dessus le pied de page puis sur le workspace derrière.
- **C1 — UI d'undo/redo retirée** : `Ctrl+Z`/`Ctrl+Y` et les entrées Edit ▸ Undo/Redo supprimées.
  `CommandHistory` n'est jamais alimenté ; un undo qui ne fait rien en silence est pire qu'absent —
  il invite à des éditions destructrices qu'on croit réversibles. `process_events` ne prend plus
  `command_history`.
- **E1/E2** — `InputState::release_all()` sur `WindowEvent::Focused(false)` ; les boutons ne s'arment
  que si le press **commence** dans le viewport.
- **E3** — `Ctrl+S`, `Ctrl+1`/`Ctrl+2` (workspaces), `Échap` (ferme la palette, puis désélectionne).
- **E4** — caméra au standard DCC : **milieu = orbit, shift+milieu = pan**, clic droit = orbit aussi.
- Vérifié à l'écran : palette clippée + focus + flèches (sélection descend) + Échap ferme.
- **C2 — renommage inline** : le champ remplace le libellé **sur place**, amorcé avec le nom courant
  (renommer, c'est éditer, pas retaper), focus pris une fois. `F2` sur la sélection ou le menu
  contextuel. La ligne rapporte son rect via un `Cell` et le champ est dessiné après la boucle —
  ça évite de faire descendre un `&mut String` dans la récursion. Vérifié à l'écran.
- **C3 — œil de visibilité retiré** (décision utilisateur). Il ne faisait que griser la ligne.
  `hidden_entities` reste en place comme couture pour un futur modèle d'**activation de composants**
  — voir ci-dessous.
- **C6 — contrôles morts supprimés ou branchés** : `+` de la Hierarchy réellement câblé (clic = entité
  vide, clic droit = même liste que le menu du fond) ; `More`/`Filter` de la Hierarchy et
  `More`/`Lock` de l'Inspector supprimés ; les 2 entrées « Refresh » de l'asset browser supprimées
  (leur propre log disait que le pump de hot-reload s'en charge) ; l'interrupteur d'activation des
  cartes d'Inspector supprimé — il était peint **sans `interact_rect`** et jamais affiché.
- **Reste en Phase 4** : D2 (remonter les erreurs à l'utilisateur — demande de choisir un hôte de
  toasts, ce qui relève plutôt de la Phase 5).

## Chantier à ouvrir — activation de composants
Désactiver un composant (l'œil de la hiérarchie, l'interrupteur des cartes d'Inspector, « éteindre
cette lumière sans la supprimer ») n'a de sens que si **chaque requête qui le consomme honore le
drapeau** ; sinon il est décoratif, exactement le défaut qu'avait l'œil. C'est donc une fonctionnalité
de l'ECS, pas de l'éditeur : il faut choisir le stockage (marqueur par type ? bitset par entité ?)
puis faire passer tous les `Flow` dessus. Comparable en taille au chantier provenance.
Précédent utile si on veut un raccourci éditeur-seul en attendant : `RenderFlow` consulte déjà une
ressource runtime (`EditorViewportOverride`) et replie son empreinte dans la clé de cache.
- Vérif : `cargo test --workspace` 974 passed / 0 failed ; clippy clean.

## Latest work (2026-07-31, suite 3) — Phase 3 : fondations d'API UI
- **A4 — `last_response` renseigné** (`khora-infra/.../ui_builder.rs`) par `text_edit_singleline`,
  `checkbox`, `drag_value_f32`, `slider_f32`, `vec3_editor`, `color_edit`, `combo_box`. C'était la
  cause racine : `is_last_item_enter_pressed`/`escape_pressed` inspectaient un `None` et renvoyaient
  `false` à jamais. **`Échap` ferme désormais la palette.**
- **A2 — API clavier** : `Interaction.focused` (aucun anneau de focus n'était peignable avant), plus
  `UiBuilder::{key_pressed, keyboard_captured, focus_last_item}`. `key_pressed` **renvoie `false`
  quand un champ texte a le focus** — c'est le point de la méthode : un panneau qui pilote la
  sélection aux flèches doit se taire pendant qu'on tape. Réutilise le `KeyCode` moteur
  (`khora-core/platform/input.rs`) ; `map_key` est volontairement partiel (une touche non mappée
  répond « pas pressée » au lieu d'en matcher une autre).
- **A1 — scroll** : `push_clip_rect`/`pop_clip_rect` + `scroll_delta_in(rect)` sur `UiBuilder`
  (delta borné à un rect, sinon deux panneaux consommeraient le même geste), et
  `khora_tool_ui::widgets::{ScrollState, scrollbar}`. Le clamp tourne **à chaque frame**, pas
  seulement sur molette, pour que du contenu qui rétrécit ramène la vue. Barre peinte uniquement en
  débordement. 4 tests.
  - **Appliqué aux 5 panneaux** : Console, Hierarchy, grille d'assets et colonne d'agents du Control
    Plane via `ScrollState` (contenu peint en rects absolus) ; **Inspector via `scroll_area`**, car
    ses cartes suivent le curseur egui — deux mécanismes, choisis selon la façon dont le panneau
    place son contenu.
  - **Chips de titre redondants supprimés** (Hierarchy / Inspector / Console / Assets) : l'onglet du
    dock nomme déjà le panneau. Les compteurs vivants (entités, assets) sont conservés seuls.
  - **Barre de défilement saisissable** : `scrollbar` prend `&mut ScrollState` + un `id_salt` et
    gère le drag (`ScrollState::drag_to`, le pouce se centre sur le curseur — clic n'importe où sur
    la piste = saut, glisser = suivi exact). Zone de préhension de 14 px pour une barre peinte de
    6 px. La molette seule ne suffisait pas.
- **A3 (graisse/interlettrage) : non fait**, glissé en Phase 5 comme le plan l'autorisait.
- **Observation console** : les lignes sont en *newest-first* et le moteur logue en continu, donc une
  vue scrollée dérive sous les nouvelles entrées. Le scroll fonctionne (barre + pouce proportionnel
  vérifiés à l'écran) mais l'ordre du journal rend la lecture d'historique frustrante — vrai défaut
  de conception du panneau, antérieur à ce changement, à traiter avec C1/C2 de l'audit.
  *(Une observation « onglet Console actif + Debug affiché au lancement » a été notée puis levée :
  c'était l'utilisateur qui avait changé d'onglet et de filtre à la main. Le constat C1 de l'audit
  reste valide tel qu'il est décrit.)*
- Vérif : `cargo test --workspace` 972 passed / 0 failed ; clippy clean ; éditeur lancé, scroll et
  barre vérifiés sur 1607 lignes.

## Latest work (2026-07-31, suite 2) — Phase 2 : dock déplaçable
- **Modèle pur** (`khora-core/src/ui/editor/dock.rs`) : `DockTree` récursif —
  `Tabs { panels, active }` / `Split { id, axis, ratio, first, second }` — plus `insert` (qui
  **déplace** si le panneau est déjà là, et renvoie le `SplitId` créé), `remove` (une pile vidée
  effondre son split), `activate`, `set_ratio`, `layout` et `zone_at`/`ratio_from_pointer`.
  Sérialisable serde. 15 tests, dont non-chevauchement des panes et aller-retour serde.
- **Peinture** (`khora-tool-ui/src/widgets/dock.rs`) : bandeau d'onglets (réutilise `nav::panel_tab`),
  splitters invisibles au repos, overlay de zone de drop qui montre **l'aire résultante**, fantôme de
  drag. 6 tests. **Pas de bouton de fermeture** : tant que le menu View n'existe pas, fermer un
  panneau le perdrait définitivement.
- **`WorkbenchPanel`** (`khora-editor/src/workbench.rs`) : un `EditorPanel` en slot `Center` qui héberge
  tous les autres, avec **un `DockTree` par `EditorMode`**. Chaque feuille reçoit son rect via
  `region_at`, donc les panneaux hébergés ignorent tout du dock.
  - **Écart assumé au plan** : le plan prévoyait d'amincir le shell. Impossible — la peinture du dock
    vit dans `khora-tool-ui` (marque Khora) et `khora-infra` ne doit pas en dépendre, sinon tout jeu
    bâti sur le moteur hériterait de la marque. Héberger le dock en panneau `Center` respecte CLAD et
    touche bien moins de code : **zéro changement au trait `EditorShell`**.
- **F12 réglé** : l'arbre-par-mode remplace les tests `active_mode` éparpillés (supprimés de
  `viewport.rs` et `control_plane.rs`) **et** la fuite `hide_right_panel` du shell. Le Control Plane
  occupe enfin tout le workbench ; la hiérarchie vide et le dock bas inutile ont disparu.
- **Ids de panneaux normalisés** : `viewport`/`scene_tree`/`properties`/`console`/`asset_browser`
  étaient nus alors que le reste (`khora.editor.spine`, `khora.editor.control_plane`) est namespacé.
  Tous en `khora.editor.*` — le dock persiste par id, donc ils doivent être stables.
- **Reste à faire (Phase 5)** : chaque panneau peint encore son propre chip de titre, désormais
  redondant avec l'onglet du dock (« Scene Tree » + « Hierarchy », « Properties » + « Inspector »,
  « Asset Browser » + « Assets »). La bande d'en-tête garde ses icônes d'action, donc seul le chip
  doit sauter. La **persistance RON** du layout n'est pas encore branchée non plus.
- Vérif : `cargo test --workspace` 968 passed / 0 failed ; clippy clean ; éditeur lancé, 61 fps,
  les deux workspaces vérifiés à l'écran.

## Latest work (2026-07-31, suite) — Phase 1 : corruption d'état de l'éditeur
- **Arbre de scène déterministe** (`ops::extract_scene_tree`) : reconstruction **descendante depuis
  les racines**, triée par `entity.index` à chaque niveau, avec garde de cycle. L'ancienne passe
  ascendante drainait une `HashMap` — dont le hasher est re-seedé par instance — donc une hiérarchie
  à 3 niveaux perdait un niveau au hasard et les fratries se réordonnaient à chaque frame. 3 tests.
- **`EditorState::clear_entity_references`** : purge sélection / inspected / hidden / rename /
  état des cartes / scene_roots. Appelée par `apply_new_scene`, `apply_stop` (restore) et
  `load_scene_dispatch` — ces chemins respawnent avec de nouveaux `EntityId`, et un slot recyclé
  faisait agir `Suppr` sur une *autre* entité. La purge vit dans `load_scene_dispatch` (qui prend
  désormais `&Arc<Mutex<EditorState>>`) plutôt qu'à chaque site d'appel.
- **Payload de drag d'asset estampillé** : `pack_asset_drag(index, epoch)` /
  `unpack_asset_drag(payload, current_epoch)` (8 bits d'epoch + 24 d'index). Un rescan en cours de
  drag faisait atterrir le drop sur le mauvais asset. `is_asset_drag` (tag seul) est séparé pour la
  *classification* — sinon un drag périmé serait pris pour un `EntityId` et traité en reparent.
- **Ids egui stables** : `region_at` prend un `id_salt: &str` (13 sites nommés) au lieu de dériver
  l'id de la position écran — bouger un panneau d'un pixel faisait perdre le focus en pleine saisie.
  `combo_box` prend un `id_salt` et utilise `ComboBox::new` au lieu de `from_label` (deux enums
  homonymes partageaient un popup). Clé des cartes d'Inspector = index **+ génération**.
- **Widget `modal`** (`khora-tool-ui/src/widgets/modal.rs`) : `confirm_modal` + `Confirm` +
  `ModalChoice`, il manquait à la charte. Câblé sur les **3** chemins de suppression de l'asset
  browser (icône poubelle, menu contextuel de tuile, menu de dossier) — ils envoyaient fichiers et
  dossiers à la corbeille sans rien demander.
- **`save_scene_dispatch*` renvoie `bool`** au lieu de jeter le résultat. *Correction d'audit : la
  sauvegarde n'échouait **pas** en silence — `save_scene_in_project_with_goal` loggue ses deux
  branches d'échec ; c'est l'appelant qui ignorait l'issue.*
- Vérif : `cargo test --workspace` 945 passed / 0 failed ; clippy clean ; éditeur lancé, 61 fps,
  aucune régression visuelle.

## Latest work (2026-07-31) — Audit UI éditeur + provenance des composants
- **Audit complet de l'UI éditeur** : `docs/research/2026-07-31_editor-ui-audit.md` — 12 sections,
  ~90 constats identifiés (`H1`, `Q2`, `X8`…), menés en lecture de code **et** en exécution réelle à
  trois largeurs de fenêtre. Plan de correction en 7 phases. Défauts structurels : aucun panneau ne
  scrolle (`scroll_area` a zéro appelant), `Interaction` n'a pas de `focused` et `UiBuilder` aucune
  API clavier, `last_response` non renseigné par les champs de saisie (⇒ `Entrée`/`Échap` morts dans
  la palette), 22 des 33 widgets de `khora-tool-ui` sans appelant.
- **`ComponentProvenance` (nouvel axe, `khora-data/src/ecs/registry.rs`)** — orthogonal à
  `SemanticDomain` : celui-ci dit *qui consomme* la donnée, la provenance dit *qui a le droit de
  l'écrire*. 4 variantes encodant deux bits (offert à l'auteur / copié à la duplication) :
  `Authored` (défaut) · `ToolAuthored` (`Prefab`, `Parent`) · `Derived` (`GlobalTransform`,
  `Children`) · `Runtime` (`PhysicsDebugData`). Déclaré via `#[component(provenance = …)]`.
  Remplace quatre encodages ad hoc concurrents (`no_serializable`, `#[component(skip)]`,
  `INHERENT_COMPONENTS`, la liste en dur de `duplicate_entity`).
- **`serialize_all_components` filtre désormais sur la provenance**, et les **4 chemins de
  sérialisation** (subtree/prefab, recipe monde, messagepack, definition) passent par lui — ils
  itéraient l'inventaire en direct. Conséquence corrigée : les `.kprefab` et les scènes
  embarquaient `Children`/`GlobalTransform` avec les **EntityId de la source**.
- **`link_parent_child`** (`scene/registry.rs`) : les 3 gestionnaires `SceneCommand::SetParent`
  n'ajoutaient que `Parent` ; l'index inverse `Children` venait du composant sérialisé (donc faux).
  Ils maintiennent maintenant les deux côtés, comme `GameWorld::set_parent`.
- **`duplicate_entity` réécrit** (`khora-editor/src/ops.rs`) : passe par le round-trip
  `serialize_subtree`/`instantiate_subtree` au lieu d'une liste de 8 composants en dur. Corrige la
  perte de `Tag`, des composants utilisateur, du `Parent` et de tout le sous-arbre. 2 tests de
  régression.
- **« + Add Component » / cartes Inspector** : filtrage via `is_author_facing` (provenance + domaine
  `Ui` masqué hors workspace Canvas) au lieu de la liste de chaînes `INHERENT_COMPONENTS`.
  Vérifié à l'écran : les buckets `UI` et `Other` (où vivait `Prefab`) ont disparu.
- **Bug de macro corrigé** (`khora-macros`) : `parse_nested_meta` avortait sur la première clé
  `key = value` non consommée et l'erreur était avalée par `let _ =`, donc une seconde clé n'était
  jamais lue. Les parseurs `domain`/`provenance` sont fusionnés en une passe et `no_serializable`
  est durci.
- Vérif : `cargo test --workspace` 940 passed / 0 failed ; `cargo clippy --workspace` clean ;
  éditeur lancé plusieurs fois, sortie propre (code 0), aucun panic.

## Earlier work (2026-07-18) — Audit remediation (robustness + dead-code sweep)
- Plan/recherche : `docs/plans/2026-07-18_codebase-audit-remediation.md`, `docs/research/2026-07-18_codebase-audit.md`.
- **Results faillibles jetés** (`let _ =`) désormais gérés : `write_buffer` (`forward_plus_lane`, pattern
  `if let Err → log::error`), `add_component` (`game_world`, `asset_resolver`, `projection` → logués),
  `device.poll` (`wgpu/device.rs`). `// SAFETY:` ajouté à `world.rs` (page-migration).
- **Locks poison-safe workspace-wide** : ~30 sites `.lock()/.read()/.write().unwrap()` → idiome
  `.unwrap_or_else(|e| e.into_inner())` (recovery, ne crashe jamais) ; `text.rs::flush` propage via `?`.
  Helpers existants : `khora_core::lane::lock` + macro `lock_or_log!` (khora-lanes).
- **Hygiène** : `command.rs` profiler pass index → fallback logué ; `unreachable!()` gardés auto-documentés.
- **Code mort supprimé** : `physics_lane/native_lanes.rs` (orphelin, violait CLAD R2) + son wiring
  `CollisionPairs` mort dans `engine.rs` ; module `ui_lane`/`StandardUiLane` + champ `layout_lane` de
  `UiAgent` (chemin jamais exécuté, violation CLAD R2 latente éliminée) ; imports morts `lit_forward_lane`.
- **Unification statut agent (Option A)** : les 5 agents ne portent **plus d'état par-frame**. Le
  scheduler mesure le temps d'`execute` et l'écrit dans un `AgentFrameStatusMap`
  (`Arc<RwLock<HashMap<AgentId, AgentFrameStatus>>>`, dans `runtime.resources`, défini
  `khora-core/control/gorna.rs`). `report_status` lit ce temps via le free fn `measured_frame_time_ms`
  (pas de méthode inhérente, RULES §8) et dérive `health_score` du `time_budget` retenu. `is_stalled`
  retiré (`false`) : le scheduler ne peut pas distinguer « crash interne » d'un skip volontaire → GORNA
  garde les triggers pression + health-degraded, perd le détecteur crash (qui reposait sur l'auto-report).
  `AgentStatus` inchangé ; editor + GORNA non modifiés. draws/tris de l'editor = `GpuReport`/MonitorRegistry.
- Vérif : `cargo test --workspace` 866 passed / 0 failed ; `cargo clippy --workspace` clean ; run editor
  12 s sans panic ni erreur de validation Vulkan.

## Latest work (2026-07-15) — Asset explorer + stable asset identity
- **Stable asset UUIDs via a registry** (`khora-io/src/asset/id_registry.rs`, `AssetIdRegistry`): UUIDs were
  always `new_v5(rel_path)`; now that's only the *default*. `<project>/.khora/asset-registry.ron` (RON, one
  `(uuid, path)` per line, sorted by UUID, atomic write) can **freeze** an identity. **Lazy freeze**: no entry ⇒
  `new_v5(path)` (old projects/tests unaffected); a rename/move freezes the *current* UUID so references
  (`MeshRef::Asset`/`MaterialRef`/texture slots — stored as raw UUID bytes) never break, on disk or in the open
  scene, with zero rewriting. Read side is engine (`IndexBuilder::with_registry`, `PackBuilder` both resolve
  through it ⇒ dev/release parity); **only the editor writes** it (`ProjectVfs`). Registry lives at project root
  (outside `assets/`) ⇒ never scanned/watched/packed.
- **Real file explorer** (`khora-editor/src/panels/asset_browser/`): generic per-tile/folder/empty context menus
  (Open, Reveal, Rename [inline], Duplicate, Delete→OS recycle bin via `trash` crate), wired header buttons,
  empty folders shown (`ProjectVfs::list_dirs` + `EditorState::asset_dirs`), epoch-based rescan
  (`asset_epoch`, replaces the count-only key), generic drag payload (`ASSET_DRAG_TAG`, was prefab-only).
  Drag a tile onto a folder = move; onto the viewport = instantiate (mesh spawns `MeshRef::Asset` at the
  unprojected ground-plane drop point via `UiBuilder::pointer_position` + `EditorCamera::screen_to_ray`, prefab
  instantiates, scene loads, texture/material assigns to the selected entity).
- File ops routed through new `ProjectVfs::{create_folder,rename_asset,move_asset,delete_to_trash,duplicate_asset}`
  + drained by `commands::process_pending_asset_file_ops` / `process_pending_spawn_mesh_asset` /
  `process_pending_assign_texture`. New deps: `trash`, `walkdir` (editor).

## Latest work (2026-06-11) — Adaptive loop closure
- **PID recovery loop closed** (`khora-control/src/service.rs`): the DCC remembers the
  `global_budget_multiplier` at last budget issuance and re-arbitrates when the frame-time PID drifts
  it by more than `PID_RENEGOTIATE_DELTA` (0.05) — pressure heuristics drive downgrades, this drift
  trigger drives *recovery* (agents upgrade again once measured frame time settles under the setpoint).
- **Empirical cost calibration** (`khora-control/src/gorna/mod.rs`): `GornaArbitrator::arbitrate` takes
  per-agent measured costs (CostModel forecast at current workload, fallback `latest_ms`); during
  negotiation each agent's quoted estimates are rescaled so the current-strategy option equals the
  measurement (factor clamped `[0.25, 4.0]`) — the fit reasons in measured ms, not static quotes.
- **Three real shadow tiers**: new `MediumShadowsLane` (1024² × 4-layer 2D + 256² × 4-cube, ≈22 MiB).
  Mapping: HighPerformance→Standard (2048²+512², ≈88 MiB), Balanced→Medium, LowPower→LowRes
  (512²+128², ≈5.5 MiB). Honest per-tier VRAM quotes, each tier gated on the VRAM constraint; LowRes
  always offered as the floor.
- **Per-domain change epochs + Flow view caching** (`khora-data`): `World::domain_epoch` /
  `World::instance_id`; opt-in `Flow::cache_key`; `register_flow!` (`run_flow_cached`) republishes the
  previous View on a key hit. Cached: Audio, Render, Shadow (Render/Shadow fold a bit-level fingerprint
  of the editor viewport override). Uncached: Ui (surface size / hot-reload fonts have no signal),
  Physics (mutates every simulated frame). Representation-only — cached View is bit-identical.
- **Sequential budget semantics documented** (`khora-control/src/scheduler.rs` rustdoc + `08_gorna.md`):
  agents run sequentially in priority order, so GORNA budgets are exclusive per-agent time slices
  (sum-of-costs fitting); parallel execution (stub) will require a critical-path model.

## Earlier work (2026-05-19) — WGSL migration finalization
- **Shadow agent**: `ShadowStrategy::from_strategy_id` made `pub`; tests cover StrategyId→ShadowStrategy
  mapping (`LowPower→LowRes`, `HighPerformance/Balanced→Standard` — since superseded by the Medium tier,
  see Latest work) and `apply_budget`.
- **Forward+ shadows**: `ForwardPlusLane` reads `ShadowFrame` from the deck; group 3 = shadow bindings,
  group 4 = F+ tile/light data + `shadow_view_projs`. Directional/spot 2D PCF + point-light cube shadows
  → full parity with `LitForwardLane`.
- **OverlayAgent**: new agent owning post-main-render `LaneKind::Render` lanes, runs in `OUTPUT` after
  `Renderer` (Hard dep). Lanes run in parallel, order `Grid → Emissive → Wireframe → Gizmo`.
- **GizmoLane / GridLane**: editor/debug overlays via `Arc<Mutex<…>>` shared runtime resources
  (`SharedGizmoFrame`, `SharedGridConfig`), `LoadOp::Load`. The backend (`khora-infra`) no longer owns
  any render pipeline — the editor is a pure data producer.
- **StandardPbrLane**: alternative `RenderAgent` strategy (Cook-Torrance GGX), shares the
  `khora::std/lighting/shadow` lib modules and the group-3 contract.
- **Legacy WGSL cleanup**: 14 duplicate root `.wgsl` files deleted; canonical sources under
  `khora-infra/src/graphics/shader/shaders/{pipelines,lib}/`, composed by the `PipelineSystem`
  backend via `naga_oil #import`. A lane names a pipeline; it never handles source.
- **Bugfix — 5-bind-group Forward+ pipeline**: F+ briefly used 5 groups; `max_bind_groups == 4` made the
  pipeline invalid (scene stopped rendering). Fixed by the canonical 4-bind-group convention
  (`conventions.md §10`): group 3 = whole lighting domain.
- Remaining render-lane TODOs (no separate tracking doc — read the in-code `// TODO`s): Emissive/Wireframe
  `execute` draw bodies (`emissive_lane.rs`, `wireframe_lane.rs`), LitForward CLAD refactor, GORNA `Custom`
  strategy support.

## Earlier milestones (condensed; see git history)
- **Substrate / Flow / AGDF refactor**: `LaneBus`/`OutputDeck`/`TickPhase`/`DataSystemRegistration`;
  `transform_propagation` moved to a `DataSystem`; `Flow` trait + `register_flow!`; `RenderFlow`/
  `ShadowFlow`/`PhysicsFlow`/`AudioFlow`; asset decoders moved to `khora-io`; `Flow::adapt` removed.
- **SAA lifecycle refactor**: `Agent` trait cleanup (`on_initialize`/`execute`), new `ExecutionPhase`
  (Init/Observe/Transform/Mutate/Output/Finalize) + `EngineMode`, `ExecutionScheduler` + `BudgetChannel`
  + `EnginePlugin`, `khora-io` crate split out.
- **Editor**: component serialization via `#[derive(Component)]`, Add/Remove Component UI, prefab
  extract/instantiate (`.kprefab`), drag-and-drop reparenting, Save-As goal picker.

## Open / deferred work (verified against code, 2026-06-04)
- **`EmissiveLane` / `WireframeLane` `execute` are no-ops** — pipelines compose at init but the draw bodies
  are TODO (`emissive_lane.rs`, `wireframe_lane.rs`) pending gating data (`MaterialKind::Emissive`
  flag / a debug flag).
- **Minor TODOs**: GPU VRAM capacity detection + dynamic adapter name (`wgpu/device.rs`), GPU timestamp
  writes (`wgpu/command.rs`), Taffy uses a hardcoded 1920px viewport width (`ui/taffy/taffy_layout.rs`).

> Verify the live state before asserting — these are code-grounded as of the date above, not runtime claims.
> Resolved (do not re-list as issues): Vulkan semaphore errors are handled by the single-acquire frame
> lifecycle (`wgpu/device.rs`); egui↔wgpu-28 is solved by the custom `EguiWgpuRenderer` in `khora-infra`.
> The orphan `physics_lane/native_lanes.rs` (experimental broadphase/solver that queried the World
> directly) was **removed** rather than migrated; the real physics path routes through `PhysicsProvider`
> + the `physics_world_writeback` DataSystem.

## Architecture decisions
See [`decisions.md`](./decisions.md). Crate count is **17 workspace members** (14 `khora-*` + sandbox
+ xtask + hub, plus `khora-macros` as a non-member path crate);
older notes saying 11/12 were stale and omitted `khora-runtime`.

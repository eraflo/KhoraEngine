## Qwen Added Memories
- ## État de la session Engine SDK Refactor — à reprendre

### Ce qui est FAIT et COMPILE:
- `InputEvent` + `MouseButton` → `khora_core::platform::input`
- `FrameContext` + `StageHandle<T>` créés dans `khora_core::renderer::api::core` (blackboard type-safe + tokio spawn + sync entre agents)
- `tokio` ajouté au workspace Cargo.toml
- `khora_core::utils::any_map::AnyMap` créé (type-erased map)
- `EngineCore` réécrit dans engine.rs (winit-agnostic, pur)
- `winit_adapters.rs` réécrit avec `WinitWindowProvider`, `WinitAppRunner`, `run_winit()`
- Editor et Sandbox migrés vers `run_winit` + bootstrap closure
- Built-in agents (Render, Shadow, UI) enregistrés automatiquement par l'engine
- `ShadowAgent` créé dans `khora-agents/src/shadow_agent/`
- `khora_editor/src/mod_agents.rs` simplifié (ne fait plus rien, les built-in sont auto-enregistrés)
- `khora_sdk/Cargo.toml` a `khora-agents` en dépendance
- `khora-agents/src/lib.rs` exporte `shadow_agent`
- `khora-agents/src/render_agent/mod.rs` a `pub mod mesh_preparation` (pour ShadowAgent)

### Ce qui RESTE À FAIRE (prochaine session):
1. **PhysicsAgent et AudioAgent**: Leurs constructeurs prennent des args (`Box<dyn PhysicsProvider>` et `Box<dyn AudioDevice>`). L'engine appelle `PhysicsAgent::new()` et `AudioAgent::new()` sans args dans engine.rs → ERREURS DE COMPILATION.
   - Solution prévue: virer les args des constructeurs, les obtenir via le service registry dans `on_initialize()`. Le type de service à insérer dans le registry doit être défini (ex: `Arc<Mutex<Box<dyn PhysicsProvider>>>`).
   - L'engine ou l'app doit insérer le provider dans les services avant le bootstrap.

2. **Hot-path tokio spawn dans les agents**: Les agents (ShadowAgent, RenderAgent) doivent utiliser `FrameContext::spawn()` au lieu d'appels synchrones. Les agents doivent:
   - Récupérer le `FrameContext` des services
   - Créer leur `StageHandle<T>` via `fctx.insert_stage()`
   - Spawner les lanes via `fctx.spawn(async move { ... })`
   - Signaler la fin via `stage.mark_done()`

3. **Intégration FrameContext dans le runner**: Le `WinitAppRunner` doit:
   - Créer le tokio runtime au bootstrap (ou dans resumed)
   - Avant chaque tick: créer un `FrameContext`, l'insérer dans les frame services
   - Après `engine.tick()`: `tokio.block_on(frame_ctx.wait_for_all())`
   - Puis present du swapchain

4. **Nettoyer le code mort**: `prepare_frame` unused dans ShadowAgent, imports inutilisés, etc.

5. **Build + tests** après toutes les corrections.

### Fichiers clés à modifier à la reprise:
- `crates/khora-sdk/src/engine.rs` — retirer PhysicsAgent::new() et AudioAgent::new() temporairement OU les corriger
- `crates/khora-agents/src/physics_agent/agent.rs` — new() sans args, provider via on_initialize
- `crates/khora-agents/src/audio_agent/mod.rs` — new() sans args, device via on_initialize
- `crates/khora-sdk/src/winit_adapters.rs` — ajouter tokio runtime + FrameContext par frame
- `crates/khora-agents/src/shadow_agent/agent.rs` — utiliser FrameContext::spawn
- `crates/khora-agents/src/render_agent/agent.rs` — idem

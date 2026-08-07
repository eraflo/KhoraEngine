# Ergon — consolidation avant le mini-jeu

## Context

Les phases 0 à 4 du langage sont livrées et vertes (1544 tests). Elles ont été
écrites en avançant, et la dette s'est accumulée : la même conversion de valeurs
écrite quatre fois, un graphe d'imports réinventé alors qu'il existait, et deux
`Flow` qui ne projettent rien du `World`.

La duplication a **déjà divergé en bug** : `Raise(e, "Hit", Vec3(…))` produit un
`ScriptValue::Vec3` côté émission, `to_register` ne le connaît pas côté
livraison → `UnsupportedArgument` → la lane traite ça comme une faute → **le
behavior cible est désactivé définitivement**.

Et en cherchant *pourquoi* les faux `Flow` existaient, on tombe sur un vrai trou
d'architecture, qui n'est pas propre au scripting et qui bride déjà le rendu.

**Specs d'implémentation** — les deux chantiers où se tromper d'interface oblige
à tout refaire ont leur contrat écrit avant le code :

- [Le pont de valeurs](./2026-08-05_spec-value-bridge.md) — la table exhaustive,
  dont l'exhaustivité *est* la correction.
- [Le contrat de contention](./2026-08-05_spec-agent-contention.md) — le trait,
  l'imposition au point d'accès, la vérification des vagues.

---

## §0 — Est-ce que Khora copie un autre moteur ?

Réponse honnête : **pas en bloc, mais Khora n'est pas non plus sans famille.**

| Élément Khora | Famille connue | Ce qui diffère réellement |
|---|---|---|
| **CRPECS** (archétype + pages, identité dissociée du stockage) | ECS archétype — chunks Unity DOTS, archétypes Bevy, tables flecs | **AGDF** : l'adaptation opère au niveau du **champ**, pas du composant. Comme le dit `docs/src/concepts/agdf.md` : les moteurs grand public stockent SoA-entre-entités mais AoS-dans-le-composant, et *« no shipping engine adapts layout online »*. |
| **Flow → View → LaneBus** | Extraction de frame — Frostbite/HDRP, `ExtractSchedule` de Bevy | **Convergence, pas emprunt.** Rien n'indique une filiation : c'est la réponse que tout moteur finit par trouver à « donner à un consommateur parallèle un instantané stable en lecture seule ». Khora y ajoute `select` + `cache_key`, la généralise à tous les domaines, et fait choisir le consommateur par un budget. |
| **OutputDeck** | Command buffers — `Commands` (Bevy), `EntityCommandBuffer` (Unity) | Rien de notable, et c'est très bien : c'est la forme juste. |
| **Lane** | Passes interchangeables — `ScriptableRenderPass` (Unity SRP), render passes Unreal | Étendu hors rendu, et **choisi par un budget** plutôt que par configuration. |
| **Agent + GORNA + DCC** | *rien d'équivalent que je connaisse* | Unreal a un Significance Manager, Unity un package Adaptive Performance (thermique → qualité). Aucun ne fait **négocier** les sous-systèmes avec un arbitre, par frame, chacun proposant des stratégies chiffrées. |
| **Ergon interruptible** | Wasmtime a le *fuel*, Lua les hooks et coroutines | Le mécanisme existe ailleurs. Ce qui est propre à Khora : un langage **de gameplay** dont la suspension est le chemin normal et dont le budget est négocié. |

**Les formes de plomberie sont convergentes — et c'est sain. La thèse est
originale.** GORNA, le DCC et l'AGDF n'ont pas d'équivalent ; ce sont eux qui
font de Khora un moteur et pas une redite. Ce qui suit ne touche pas à la
thèse : ça finit une plomberie que Khora avait commencée et laissée à moitié.

> **Une mise en garde méthodologique, apprise en écrivant ce plan.** J'ai
> d'abord conclu que `Flow` venait de Bevy, et qu'AGDF avait été abandonné en
> route. Les deux étaient faux : rien n'atteste la première filiation, et
> `docs/src/concepts/agdf.md` décrit **trois couches livrées dans l'ordre** —
> L1 observer/conseiller et L2 substrat field-SoA sont livrées, L3 le repack en
> ligne est *« a documented frontier, deliberately deferred »*. J'avais lu un
> instantané du code et j'en avais déduit une intention. **Lire la doc de vision
> avant de diagnostiquer une dérive** — sinon on invente la dérive qu'on
> cherchait.

> **Discriminant** — emprunter les mécanismes qui donnent au DCC plus
> d'information sur ses agents ; refuser ceux qui retirent à l'agent son pouvoir
> de décision.
>
> Bevy déclare les accès pour trancher une question **statique** (qui peut
> tourner en parallèle, calculé une fois). Khora pose une question **dynamique**
> (avec 0,4 ms et des agents qui savent se dégrader, quoi, dans quel ordre, à
> quelle qualité). L'accès déclaré est une **entrée** de GORNA, pas un
> substitut. Adopter « un système est une fonction dont l'ordonnanceur
> introspecte la signature » dissoudrait l'Agent — et l'Agent *est* SAA.

---

## §1 — L'architecture aujourd'hui

```
┌─ COLD PATH ────────────────────────────── fil DCC, ~ tick_rate Hz ───────────┐
│                                                                              │
│   TelemetryEvent ──►  MetricStore  ──►  HeuristicEngine                       │
│        ▲                                     │                                │
│        │                              CostModel (c·f(n)) par agent            │
│        │                              PID frame-time ──► multiplicateur       │
│        │                                     ▼                                │
│        │                            GornaArbitrator                           │
│        │                     (négociation : chaque agent propose des          │
│        │                      StrategyOption {temps, VRAM} ; l'arbitre        │
│        │                      alloue selon priorité + AdaptationMode)         │
│        │                                     │                                │
│        │                                     ▼  budget_channel                │
└────────┼─────────────────────────────────────┼───────────────────────────────┘
         │                                     │  ResourceBudget
         │ AgentCost, AgentStatus              │
┌────────┼─────────────────────────────────────┼───── HOT PATH, chaque frame ──┐
│        │                                     ▼                               │
│  ┌─────┴──────────────────────────────────────────────────────────────────┐  │
│  │ 1. DataSystems  PreSimulation → PostSimulation → PreExtract            │  │
│  │    (inventory, phases ordonnées ; deck transitoire, jeté)              │  │
│  └────────────────────────────────────────────────────────────────────────┘  │
│                                     │                                        │
│  ┌──────────────────────────────────▼─────────────────────────────────────┐  │
│  │ 2. SUBSTRATE PASS — bus et deck neufs pour la frame                    │  │
│  │    World ──[ Flow::select + project ]──► LaneBus  (vues read-only)     │  │
│  └────────────────────────────────────────────────────────────────────────┘  │
│                                     │                                        │
│  ┌──────────────────────────────────▼─────────────────────────────────────┐  │
│  │ 3. SCHEDULER — vagues, par ExecutionPhase                              │  │
│  │    INIT → OBSERVE → TRANSFORM → MUTATE → OUTPUT → FINALIZE             │  │
│  │                                                                        │  │
│  │    dans une phase, une vague concurrente =                             │  │
│  │      n agents Isolated  (WorkerPool, WorldAccess::None, shard privé)   │  │
│  │    + AU PLUS UN SharedWorld (inline, &World, deck partagé)             │  │
│  │    un agent Exclusive ⇒ vague solitaire (&mut World)                   │  │
│  │                                                                        │  │
│  │    chaque agent :  apply_budget(fuel/temps)                            │  │
│  │                    choisit une Lane parmi son LaneRegistry             │  │
│  │                    Lane::execute(LaneContext)                          │  │
│  │                       lit   ◄── LaneBus (Ref<View>)                    │  │
│  │                       écrit ──► OutputDeck (slots typés)               │  │
│  └────────────────────────────────────────────────────────────────────────┘  │
│                                     │                                        │
│  ┌──────────────────────────────────▼─────────────────────────────────────┐  │
│  │ 4. DataSystems  Maintenance — drainent le deck vers le World           │  │
│  │    (WorldCommand, writebacks physique / audio / script)                │  │
│  └────────────────────────────────────────────────────────────────────────┘  │
│                                                                              │
│  ═════ hors de tout ça : Runtime { services, backends, resources } ═════      │
│  Poignées et état partagé, atteints librement par n'importe quel agent.      │
│  Le scheduler n'en sait RIEN.   ◄── le trou                                  │
└──────────────────────────────────────────────────────────────────────────────┘
```

Le `Runtime` est le seul élément que la descente CLAD traverse sans que
l'ordonnanceur en ait la moindre visibilité. C'est là qu'est le problème.

---

## §2 — Le diagnostic : la moitié manquante d'un contrat

`ScriptReloadFlow` et `ScriptEventFlow` ne lisent pas le `World`. Ils existent
parce que l'agent déclare `AgentAccess::Isolated`, qui interdit de toucher une
ressource partagée — et la seule façon d'atteindre un agent `Isolated` est le
bus, où seuls les `Flow` écrivent.

Mais **pourquoi** `Isolated` interdit-il toutes les ressources d'un bloc ? Parce
que le scheduler ne sait pas *lesquelles*. Ce qu'un agent déclare aujourd'hui :

| Déclaration | Existe ? | Vérifiée ? |
|---|---|---|
| `access()` — son rapport au `World` | oui | oui (construction des vagues) |
| `deck_writes()` — ce qu'il écrit sur le deck | oui | oui (`check_wave_deck_disjoint`) |
| ce qu'il lit ou verrouille dans `runtime.resources` / `services` | **rien** | — |

`AgentAccess` est le bouche-trou de cette asymétrie.

**Ce n'est pas propre au scripting — et ça coûte déjà, ailleurs :**

- Aucun agent, lane ou data-system ne lit `InputMap`. Le moteur publie une carte
  d'entrée que la descente CLAD ne consomme pas. La tâche #11 heurtera ce mur.
- `RenderAgent` et `UiAgent` sont **tous deux `SharedWorld`, tous deux en
  OUTPUT**. La règle « au plus un `SharedWorld` par vague » leur interdit
  aujourd'hui de partager une vague — alors que leur contention réelle est
  peut-être nulle. On sérialise deux agents faute de savoir sur quoi ils se
  marchent dessus.

### Ce qu'on ne fait pas : toucher au `World`

Mettre les ressources dans le `World` rendrait les `Flow` honnêtes. Non :

- CRPECS est **à propos de** la dissociation identité/stockage pour l'AGDF. Une
  ressource n'est pas une entité, n'a pas de stockage de composants, n'a rien à
  adapter en layout. On lui donnerait une responsabilité que sa conception ne
  sert pas.
- Elle deviendrait candidate à `SerializationGoal` — un `Program` compilé dans
  les fichiers de scène.
- Flecs met ses singletons dans le monde parce que son ordonnanceur raisonne sur
  l'accès **composant**. Celui de Khora raisonne sur les agents et le deck. On
  importerait le risque sans le bénéfice.

`World` reste la scène. `Runtime::resources` est déjà documenté « long-lived
shared state (`InputMap`, …) » : la catégorie est juste, c'est sa **visibilité
par l'ordonnanceur** qui manque.

---

## §3 — La correction : déclarer, vérifier, imposer

### 3.1 Ce qu'un agent déclare

`deck_writes` est absorbé dans une déclaration unique, pour que la contention
d'un agent se lise en un endroit :

```rust
// khora-core/src/agent/contention.rs
#[derive(Default, Clone)]
pub struct Contention {
    /// Slots de l'OutputDeck écrits (remplace `deck_writes`).
    pub deck: Vec<TypeId>,
    /// Ressources/services lus sans verrou exclusif.
    pub reads: Vec<TypeId>,
    /// Ressources/services que l'agent VERROUILLE pour muter ou drainer.
    pub writes: Vec<TypeId>,
}

// trait Agent
fn contention(&self) -> Contention { Contention::default() }
```

> **Pourquoi `writes` et non `&mut`** : `Resources::get<T>()` ne rend qu'un
> `&T`, et la mutation passe par l'intérieur (`Arc<Mutex<InputMap>>`). « Écrire »
> signifie donc **verrouiller** — ce qui est exactement la contention qui
> compte : deux agents concurrents qui verrouillent le même mutex se
> sérialisent, et le scheduler doit le savoir avant de les mettre dans la même
> vague, pas le découvrir en le mesurant.

### 3.2 Ce que le scheduler vérifie

Dans une vague concurrente : `writes ∩ writes = ∅` et `writes ∩ reads = ∅`,
entre agents. Même forme et même fichier que `check_wave_deck_disjoint`
(`khora-control/src/scheduler.rs:1020`), qui devient un cas de la nouvelle
vérification.

### 3.3 Ce que le contexte impose

L'agent n'atteint plus `runtime.resources` directement :

```rust
// EngineContext
pub fn resource<T: Send + Sync + 'static>(&self) -> Option<&T>;   // doit être dans reads ∪ writes
pub fn locked<T: Send + Sync + 'static>(&self) -> Option<&T>;     // doit être dans writes
```

Le scheduler estampille le contexte avec la `Contention` de l'agent avant
`execute`. Un accès non déclaré est refusé (log `error` + `None`), et la
vérification est active en release : elle coûte une recherche dans un vecteur de
trois éléments.

**C'est l'idiome que `EngineContext` s'impose déjà à lui-même** pour le `World`
— *« reach it through `world_ref`/`world_mut`, never by matching directly »*. On
l'étend, on ne l'invente pas. Et ça corrige la faiblesse de `deck_writes`, qui
reste sur l'honneur : exactement la classe de bug déjà trouvée sur cet agent.

### 3.4 Ce que `AgentAccess` devient

Il ne parle plus que du `World` :

| Variante | Sens après |
|---|---|
| `Exclusive` | `&mut World` — vague solitaire |
| `SharedWorld` | `&World` en lecture |
| `Isolated` | ne touche pas le `World` |

**Et la règle « au plus un `SharedWorld` par vague » tombe.** Elle existait parce
que ces agents « may write shared resources » sans qu'on sache lesquelles. Une
fois déclarées, plusieurs `SharedWorld` peuvent partager une vague si leurs
ressources sont disjointes — `RenderAgent` et `UiAgent` en OUTPUT deviennent
candidats à la concurrence. **C'est un gain de parallélisme obtenu en retirant
une contrainte, pas en ajoutant un mécanisme.**

### 3.5 L'architecture après

```
  disque (.erg)   clavier / OS   contacts physiques (à venir)
        │              │                   │
        └──────────────┴───────────────────┘
                       ▼
        Runtime { resources, services }
                       │
                       │  EngineContext::resource / locked
                       │  refuse ce qui n'est pas déclaré
                       ▼
   ┌──────────────────────────────────────────────────┐
   │  Agent                                           │
   │    access()      → le World, et rien d'autre     │
   │    contention()  → deck | reads | writes  ◄─NEW  │
   └──────────────────────────────────────────────────┘
              ▲
              │  scheduler : une vague concurrente
              │  ⇒ deck, reads et writes disjoints
              │  ⇒ plus de limite « un seul SharedWorld »

   World ──Flow──► LaneBus ──► Agent ──► OutputDeck ──DataSystem──► World
          (et Flow ne fait plus que ça)
```

### 3.6 Ce que ça débloque

- Les deux faux `Flow` disparaissent ; `Flow` retrouve sa définition **par
  soustraction** — plus aucune implémentation n'ignore ses paramètres `World`.
- `ScriptReload { module, program }` → `khora-script`.
  `PendingScriptReloads` → `khora-io`. `PendingScriptEvents` →
  `khora-core/src/script/`. **La dépendance `khora-data` → `khora-script`
  disparaît.**
- L'agent de script **reste `Isolated`** et déclare
  `writes: [PendingScriptReloads, PendingScriptEvents]`. Il perd son champ
  `inbox` : une seule file au lieu de deux.
- `InputMap` devient consommable par un agent `Isolated` qui le déclare en
  lecture — tâche #11 débloquée.
- `RenderAgent` + `UiAgent` peuvent partager une vague.
- Le producteur de collisions a sa route sans rien de neuf.

### 3.7 Audit d'obsolescence

Ce que la correction rend caduc, et ce qu'elle laisse intact :

| Élément | Sort |
|---|---|
| `Agent::deck_writes()` | **absorbé** par `contention().deck` — 4 agents à migrer |
| « au plus un `SharedWorld` par vague » | **supprimé** (§3.4) |
| `ScriptReloadFlow`, `ScriptEventFlow` | **supprimés** |
| `ScriptingAgent::inbox` | **supprimé** |
| `check_wave_deck_disjoint` | **généralisé**, pas supprimé |
| `AgentAccess` | **conservé**, sens rétréci au `World` |
| `WorldAccess`, `Flow`, `LaneBus`, `OutputDeck`, `Lane`, `DataSystem` | **inchangés** |
| GORNA, DCC, `ResourceBudget`, PID, `CostModel` | **inchangés** — la contention est une contrainte de *placement en vague*, pas de budget |
| `WorkerPool` | **inchangé** — les jobs `Isolated` reçoivent déjà `Arc<Runtime>` |
| Substrate Pass, `TickPhase`, `ExecutionPhase` | **inchangés** |

**À traiter explicitement, sinon ça mordra :** `AgentFrameStatusMap` est écrit
par le scheduler et lu par les agents dans `report_status`. Il est
**engine-owned** : exempté de la déclaration, et le dire dans la doc plutôt que
laisser chacun deviner.

**Rien d'autre ne devient obsolète.** La correction ajoute une déclaration et
retire une contrainte ; elle ne remplace aucune brique existante.

---

## §4 — Un seul pont de valeurs, piloté par une table

Quatre conversions à la main sur le même univers :

| Fichier | Fonction | Couvre |
|---|---|---|
| `khora-script/src/dispatch.rs:387` | `to_register` | Unit, Bool, Int, Float, Entity |
| `khora-script/src/native/events.rs:87` | `carried` | + Vec3, Str |
| `khora-lanes/src/script_lane/persistence.rs:304` | `to_persisted` / `to_script_value` | + Vec3, Str |
| `khora-data/src/ecs/systems/script_commands/json.rs:36` | `to_json` | tout |

Une table déclarative unique en X-macro dans `khora-core/src/script/table.rs` :

```rust
#[macro_export]
macro_rules! script_value_table {
    ($callback:path) => { $callback! {
        //  variante  type Rust    stockage  type Ergon
        Bool    : bool           , inline  , Bool            ;
        Int     : i64            , inline  , Int             ;
        Float   : f32            , inline  , Float           ;
        Entity  : EntityId       , inline  , Entity          ;
        Vec2    : Vec2           , inline  , Engine("Vec2")  ;
        Vec3    : Vec3           , inline  , Engine("Vec3")  ;
        Vec4    : Vec4           , inline  , Engine("Vec4")  ;
        Quat    : Quaternion     , inline  , Engine("Quat")  ;
        Color   : LinearRgba     , inline  , Engine("Color") ;
    }};
}
```

Un module `khora-script/src/bridge.rs` — le **seul** endroit qui connaît à la
fois `Value`, `Persisted` et `ScriptValue` — consomme la table et génère les
conversions ; les quatre fonctions deviennent des appels au pont.

```
AVANT                                    APRÈS
ScriptValue ─to_register─► Value          script_value_table!  (khora-core)
ScriptValue ◄──carried──── Value                    │
ScriptValue ◄to_persisted► Persisted        ┌───────┼───────┐
ScriptValue ──to_json────► Json             ▼       ▼       ▼
       ↑ les deux premières            bridge.rs value.rs json.rs
         divergent = le bug                 │
                                   Value ◄─► ScriptValue ◄─► Persisted
```

**Ce que la table ne génère pas** : les *définitions* d'enum, dont chaque
variante porte une prose qui explique un choix. Et les cas irréguliers — `Str`
(référence / possédée / `Object::Str`), `Array`, `Struct` — restent des bras
explicites, visibles **comme** exceptions plutôt que noyés dans onze bras
identiques.

Ajouter `Quat` passe de ~8 endroits à 2.

## §5 — `#[ergon_type]`

Exposer `Vec3` a coûté un `impl ScriptType`, un natif constructeur, et quatre
bras de `match`. Une macro dérive dans `khora-macros`, sœur de `#[ergon_fn]` :

```rust
#[ergon_type(name = "Vec3")]
struct Vec3 { x: f32, y: f32, z: f32 }
```

génère l'`impl ScriptType`, le constructeur `Vec3(x, y, z)` et les accesseurs
`X(v)` / `Y(v)` / `Z(v)`, soumis à `inventory`. `Quat`, `Color`, `Vec2`, `Vec4`
deviennent gratuits. Appliquée sur une déclaration miroir dans
`khora-script/src/native/engine_types.rs`, pas sur les structs de
`khora_core::math` — un type moteur ne porte pas d'annotation scripting.

## §6 — Réutiliser le graphe d'imports

`khora-io/src/asset/dependencies.rs` extrait **déjà** les imports d'un `.erg`,
avec son `script_root()`, et sa doc dit qu'il existe *pour* le hot-reload.
`script_hot_reload.rs` l'ignore : `importers_of` re-parcourt l'arbre et
**re-parse chaque fichier** à chaque sauvegarde, et `script_root_of` est une
seconde implémentation subtilement différente.

Supprimer `script_root_of` ; remplacer `importers_of` par une marche arrière sur
les dépendances que l'index connaît ; ne garder qu'un parcours de répertoire
(`modules_under` en `read_dir` manuel et `importers_of` en `walkdir` coexistent
à 70 lignes d'écart).

> **Corrigé à l'implémentation — la marche arrière sur l'index n'est pas
> faisable.** Vérification faite, l'index ne connaît pas ces arêtes au moment
> utile : éditer un `.erg` en place ne le réindexe **jamais**. L'éditeur traite
> un `Modified` par `AssetService::invalidate` seul (`hot_reload.rs:60`) et ne
> reconstruit que sur `Created`/`Removed` ; côté runtime, `reindex` n'a qu'un
> seul appelant dans tout le dépôt, et c'est l'éditeur. Les imports que l'index
> porte sont donc ceux du dernier create/delete — un auteur qui **ajoute** un
> `import` et sauvegarde produit une arête que l'index n'a jamais vue, et son
> importateur ne recompilerait pas. Lire l'arbre est plus lent et juste ; lire
> l'index serait plus rapide et faux.
>
> L'erreur vient d'avoir cru une doc plutôt que le câblage : `dependencies.rs`
> annonce « this is what makes hot-reload correct rather than merely fast », et
> `script_hot_reload.rs` affirmait « derived from the same import lists the asset
> index already extracts ». Les deux décrivaient une intention jamais branchée.
> C'est la deuxième fois dans ce plan (cf. §0) qu'un diagnostic tiré d'un
> commentaire s'avère faux — **la doc dit l'intention, le câblage dit l'état.**
>
> Ce qui a effectivement été fait, sans l'index :
>
> - `importers_of` devient **pure** — elle prend le graphe, plus un répertoire.
>   L'ancienne re-parcourait l'arbre et relisait *chaque* fichier à **chaque
>   tour** du point fixe ; passer une carte lue une fois rend la répétition
>   impossible au lieu de simplement l'éviter. La fermeture se teste désormais
>   sans disque.
> - `modules_under` rend le graphe complet (module → imports) et sert les deux
>   appelants ; le second parcours disparaît, et le survivant est le `walkdir`
>   qu'utilise déjà l'index builder.
> - `script_root_of` n'est plus une seconde implémentation : il appelle
>   `dependencies::script_root` et retire la barre finale, la seule différence
>   entre les deux usages.
>
> **Découvert au passage, hors périmètre :** le hot-reload `.erg` est inerte dans
> l'éditeur — `script_hot_reload_system` exige un `Arc<AssetWatcher>` dans
> `runtime.resources`, et l'éditeur n'en insère aucun (le sien est un champ de
> `ProjectVfs`). Il ne fonctionne aujourd'hui que sous `run_default`.

## §7 — Découper `script_lane/mod.rs`

770 lignes : le `Lane`, le rapport, `Fuel`, `run_behaviors` (197 l.), `run_one`
(211 l.), `Hook`, `call_hook`, `say_goodbye`, `despawns_itself`,
`farewell_departed`, `waiting_again`, `delta_moved_a_countdown`, `Outcome`,
`Invocation`.

| Fichier | Contenu |
|---|---|
| `mod.rs` | le `Lane`, `Fuel`, les ré-exports |
| `report.rs` | `ScriptRunReport` |
| `frame.rs` | `run_behaviors` — la boucle et la comptabilité |
| `turn.rs` | `run_one`, `Invocation`, `Outcome` |
| `hooks.rs` | `Hook`, `call_hook`, `say_goodbye`, `despawns_itself` |

## §8 — Code mort

`load_module` (0 usage) ; `CommandBuffer::merge_ordered` (0 usage, le scheduler
replie déjà les shards dans l'ordre de vague).

---

## Méthode : spécifier et tester d'abord — mais pas partout

### Où ce plan vit

Dans `docs/plans/2026-08-05_ergon-consolidation.md`, versionné avec le code
qu'il décrit — c'est l'artefact que la skill `create-plan` désigne, et
`docs/plans/` existe déjà. Pas un document temporaire ailleurs : un plan qui ne
survit pas au travail qu'il décrit ne sert qu'une fois.

### Spécifier avant de coder : deux chantiers sur six

Une spec gagne sa place quand se tromper d'interface oblige à tout refaire, ou
quand l'exhaustivité *est* la correction. Ailleurs, c'est de la cérémonie.

| Chantier | Spec avant ? | Pourquoi |
|---|---|---|
| **1** — pont de valeurs | **Oui** | La table **est** la spec : les 9 variantes × 4 directions (`ScriptValue` ↔ `Value` ↔ `Persisted` ↔ `Json`), plus ce que font les irrégulières (`Str`, `Array`, `Struct`). C'est exactement l'exhaustivité dont l'absence a produit le bug Vec3/string. L'écrire d'abord le rend impossible par construction. |
| **3** — contrat de contention | **Oui** | Change le trait `Agent`, le scheduler, `EngineContext` et 5 agents. Se tromper d'interface = refaire la migration. À figer : signatures exactes, ce que rend `resource()` sur un accès non déclaré, la vérification est-elle active en release, comment le scheduler estampille le permis, et ce qui est **exempté** (`AgentFrameStatusMap`, engine-owned). |
| 2 — `#[ergon_type]` | Non | Une macro se spécifie mal dans l'abstrait. Ce qu'on écrit d'abord, c'est son **usage** : le script Ergon qui appelle `Quat(…)`. |
| 4, 5, 6 | Non | Mécaniques. Le plan en dit déjà assez. |

### Tests d'abord : quand le test est une définition

**Sur ce projet, l'ordre inverse a coûté.** Les tests écrits *après* le code ont
trouvé de vrais bugs à chaque fois — les deux lacunes de `state`, la collision
`Unit` entre « épuisé » et « jamais armé », `OnSpawn` rejoué au chargement. Ces
bugs existaient **parce que** le code venait en premier. Ce n'est pas une
doctrine, c'est le relevé de cette session.

Mais tests-d'abord ne s'applique pas uniformément :

| Chantier | Tests d'abord ? | Le filet de sécurité |
|---|---|---|
| **1** | **Oui** | `Raise(e, "Hit", Vec3(…))` **échoue aujourd'hui** — vrai rouge-vert, et il épingle le bug. Le round-trip par ligne de table définit le contrat de la table. |
| **3a** | **Oui** | « un accès non déclaré est refusé » et « deux agents d'une vague sur la même ressource sont détectés » **sont** la sémantique. Les écrire force les décisions d'API au lieu de les découvrir. |
| **4** | Oui | Comportemental et exprimable sans le code. |
| **2** | L'usage d'abord | Écrire le script qui appelle `Quat(…)` et le laisser ne pas compiler. |
| **5** | **Non** | L'invariant, ce sont les **67 tests existants qui ne doivent pas changer**. En ajouter serait du bruit. |
| **6** | Non | Supprimer du code mort n'a pas de sémantique. |

Deux filets différents, et il faut savoir lequel protège quoi : pour 1, 3 et 4
ce sont les **nouveaux tests écrits d'abord** ; pour 5 et 6 c'est la
**non-modification des 1544 existants**.

### Le risque qu'aucun test ne rattrape tout seul

Le chantier 3 touche la construction des vagues. Une déclaration **trop large**
échoue en silence *dans le sens de la lenteur* : tout se sérialise, rien ne
casse, aucun test ne rougit, le moteur ralentit. C'est le pire mode de panne du
plan.

Parade : le test de 3b n'assertent pas seulement « la collision est détectée »
mais **la composition des vagues** — `RenderAgent` et `UiAgent` doivent se
retrouver dans la même vague concurrente. Une sur-déclaration le fait rougir.

## Ordre et commits

| # | Commit | Contenu |
|---|---|---|
| 0 | `docs` | ce plan dans `docs/plans/`, plus les deux specs (table de valeurs, contrat de contention) |
| 1 | `refactor(script)` | table + `bridge.rs`, les 4 conversions deviennent des appels — corrige le bug Vec3/string |
| 2 | `feat(script)` | `#[ergon_type]`, `Vec3`/`Quat`/`Color`/`Vec2`/`Vec4` |
| 3a | `feat(core)` | `Contention`, `EngineContext::resource`/`locked`, vérification scheduler généralisée |
| 3b | `refactor(agents)` | les 5 agents déclarent ; `deck_writes` absorbé ; règle « un `SharedWorld` » retirée |
| 3c | `refactor(script)` | les 2 faux `Flow` supprimés, déménagements, `inbox` supprimé, dépendance `khora-data → khora-script` retirée |
| 4 | `refactor(io)` | graphe d'imports via `dependencies.rs` |
| 5 | `refactor(lanes)` | découpage de `script_lane` |
| 6 | `chore` | code mort |

Chacun vert. 3a avant 3b avant 3c ; le reste est indépendant.

## Vérification

Porte complète après **chaque** commit :

```bash
cargo fmt --all && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --workspace --all-features --locked && cargo test --workspace --doc --all-features --locked
```

Les 1544 tests existants restent verts **sans être réécrits** — un test qu'il
faut changer signale un changement de comportement, pas de rangement. Exception
attendue et unique : les tests de `deck_writes` suivent son absorption (3b).

| Commit | Test décisif |
|---|---|
| 1 | `Raise(e, "Hit", Vec3(1,0,0))` traverse la livraison et le handler le reçoit — le bug d'aujourd'hui |
| 1 | round-trip `ScriptValue` → `Value` → `Persisted` → `ScriptValue` pour **chaque** ligne de la table |
| 2 | un type déclaré avec `#[ergon_type]` est constructible et lisible depuis un script, zéro ligne à la main |
| 3a | un accès non déclaré est refusé ; deux agents d'une vague qui verrouillent la même ressource sont détectés et nommés |
| 3b | `RenderAgent` et `UiAgent` se retrouvent dans la même vague concurrente si leurs ressources sont disjointes |
| 3c | un reload et un événement moteur atteignent l'agent `Isolated` ; **aucune implémentation de `Flow` du workspace n'ignore ses paramètres `World`** |
| 4 | éditer un module recompile ses importateurs **via l'index d'assets**, sans re-parser l'arbre |
| 5 | aucun test ajouté — les 67 tests de la lane prouvent l'invariance |

Puis, avant le mini-jeu : `cargo run -p sandbox` démarre, charge les scripts, et
éditer un `.erg` prend effet à chaud.

**Documentation à mettre à jour** (règle « documenter dans les fichiers
existants, pas d'ADR séparé») : `.agent/engine/RULES.md` §5 (la règle de
concurrence change), `architecture.md` (le contrat de contention), et la doc du
trait `Agent`.

---

## Ce qui reste après

**Suites directes de cette consolidation :**

- **Producteur d'événements de collision** : les contacts arrivent dans
  `CollisionEvents` indexés par `ColliderHandle` ; il faut la table
  handle→entité de Rapier (`khora-infra`).
- **Tâche #11** — input de l'éditeur sur l'`InputMap`, débloquée par §3.
- **Le mini-jeu** dans `examples/sandbox`, tout le gameplay en Ergon, plus le
  banc `criterion` (1000 behaviors sous 0,5 ms).

**Trajectoire de la vision — leurs propres plans, après le mini-jeu :**

- **AGDF couche 3 — le repack en ligne.** *Ce n'est pas une réparation* : L1 et
  L2 sont livrées, L3 est documentée comme une frontière délibérément différée.
  Le travail réel : relier les deux vocabulaires (`LayoutRecommendation` de
  l'advisor ↔ `LayoutPolicy` du registre — aujourd'hui disjoints, et
  `set_layout` a zéro appelant), implémenter `AoSoA`, et migrer une colonne
  peuplée à chaud sans casser les requêtes en cours. Le mini-jeu fournira
  justement la charge réelle qui manque pour mesurer si ça paie.

- **GORNA — de l'allocation par coût à l'allocation par valeur.** Le pas suivant
  de la vision, pas un correctif. Aujourd'hui un agent déclare ce qu'il *coûte*
  (`StrategyOption { estimated_time, estimated_vram }`) et l'arbitrage pondère
  coût × priorité **statique** (le `1.0` de `register_agent`). Rien ne permet à
  un agent de **justifier pourquoi il mérite le budget plutôt qu'un autre** —
  « cette frame, ma qualité compte plus que celle du skybox ». Ajouter une
  utilité déclarée transforme l'arbitrage en vraie allocation : maximiser la
  valeur sous contrainte, au lieu d'ordonner des priorités figées.

  *Note liée* : une stratégie n'a pas à être une lane. Pour les agents qui ne
  peuvent pas en avoir plusieurs, une stratégie est un **paramètre** — le fuel
  du scripting, la résolution d'ombre, le nombre de voix audio. La doc GORNA
  l'autorise déjà ; c'est une dégradation légitime, pas un cas dégradé.

- **Passe de documentation, à la fin.** Une fois ces chantiers passés, relire les
  docs de concepts et les mettre au niveau de ce qui tourne — la doc de
  `khora-data/src/ecs/mod.rs` en particulier, qui annonce AGDF sans dire quelle
  couche est livrée.

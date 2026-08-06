# Spec — le contrat de contention d'un agent

> Contrat d'implémentation du chantier 3 de
> [la consolidation](./2026-08-05_ergon-consolidation.md).
> Il change le trait `Agent`, l'ordonnanceur, `EngineContext` et cinq agents. Se
> tromper d'interface oblige à refaire la migration : d'où cette spec.

## Ce qui manque, exactement

Un agent déclare aujourd'hui deux choses, et l'ordonnanceur vérifie les deux :

| Déclaration | Vérifiée par |
|---|---|
| `access()` — son rapport au `World` | la construction des vagues |
| `deck_writes()` — les slots qu'il écrit | `check_wave_deck_disjoint` |

Il ne déclare **rien** de ce qu'il lit ou verrouille dans `Runtime`. Faute de
savoir *lesquelles*, `AgentAccess::Isolated` les interdit toutes en bloc — et
c'est la seule raison pour laquelle le scripting a fabriqué deux faux `Flow`
pour atteindre le bus.

Ce n'est pas propre au scripting :

- **Aucun agent, lane ou data-system ne lit `InputMap`.** Le moteur publie une
  carte d'entrée que la descente CLAD ne consomme pas. La tâche #11 heurtera ce
  mur.
- **`RenderAgent` et `UiAgent` sont tous deux `SharedWorld`, tous deux en
  `OUTPUT`.** La règle « au plus un `SharedWorld` par vague » leur interdit de
  partager une vague, alors que leur contention réelle est peut-être nulle. On
  sérialise deux agents faute de savoir sur quoi ils se marchent dessus.

---

## Le contrat

### Ce qu'un agent déclare

```rust
// khora-core/src/agent/contention.rs

/// Ce sur quoi un agent entre en concurrence avec les autres.
///
/// Une seule déclaration : les trois surfaces d'un agent se lisent au même
/// endroit, et `deck_writes` cesse d'être la seule à exister.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Contention {
    /// Slots de l'`OutputDeck` que l'agent écrit.
    pub deck: Vec<TypeId>,
    /// Ressources et services atteints en lecture, sans verrou exclusif.
    pub reads: Vec<TypeId>,
    /// Ressources et services que l'agent **verrouille** pour muter ou drainer.
    pub writes: Vec<TypeId>,
}

// trait Agent
fn contention(&self) -> Contention { Contention::default() }
```

`deck_writes()` disparaît, absorbé par `contention().deck`.

> **Pourquoi `writes` et non `&mut`.** `Resources::get<T>()` ne rend qu'un
> `&T` : la mutation passe par l'intérieur (`Arc<Mutex<InputMap>>`,
> `Arc<RwLock<Time>>`). « Écrire » veut donc dire **verrouiller** — et c'est
> exactement la contention qui compte, puisque deux agents concurrents sur le
> même mutex se sérialisent. L'ordonnanceur doit le savoir *avant* de les mettre
> dans la même vague, pas le mesurer après.

### Ce que l'ordonnanceur vérifie

Dans une vague concurrente, entre agents pris deux à deux :

| Règle | Raison |
|---|---|
| `deck ∩ deck = ∅` | shards privés repliés ensuite : un slot partagé se perdrait au repli |
| `writes ∩ writes = ∅` | deux verrous concurrents = sérialisation qu'on croyait éviter |
| `writes ∩ reads = ∅` | un lecteur verrait un état à moitié écrit |
| `reads ∩ reads` | **autorisé** — c'est tout l'intérêt |

`check_wave_deck_disjoint` (`khora-control/src/scheduler.rs`) devient le cas
`deck` d'une vérification unique qui nomme les deux agents et la ressource,
comme aujourd'hui.

### Ce que le contexte impose

L'agent n'atteint plus `runtime.resources` ni `runtime.services` directement :

```rust
// EngineContext
/// Une ressource déclarée en lecture ou en écriture.
pub fn resource<T: Send + Sync + 'static>(&self) -> Option<&T>;

/// Une ressource déclarée en écriture — celle qu'on s'apprête à verrouiller.
pub fn locked<T: Send + Sync + 'static>(&self) -> Option<&T>;
```

| Situation | Réponse |
|---|---|
| déclarée, présente | `Some(&T)` |
| déclarée, absente du `Runtime` | `None` — cas ordinaire (pas de watcher en build packagé) |
| **non déclarée** | `None` **+ `log::error!` nommant l'agent, le type et l'appel** |
| `locked` sur un type déclaré en `reads` seulement | idem : `None` + erreur |

**Actif en release.** Le coût est une recherche linéaire dans un `Vec<TypeId>`
de trois entrées, une fois par accès, hors boucle chaude — négligeable devant ce
qu'un accès non déclaré coûterait à trouver. Une vérification qui ne tourne
qu'en debug est une vérification qu'on ne croit pas.

`backends` reste libre d'accès : ce sont des poignées immuables
(`Arc<dyn GraphicsDevice>`), sans contention.

### Comment le permis arrive

L'ordonnanceur estampille le contexte avec la `Contention` de l'agent avant
`execute` :

```rust
pub struct EngineContext<'a> {
    // … world, runtime, bus, deck
    /// Ce que l'agent en cours a déclaré. Posé par l'ordonnanceur.
    permit: &'a Contention,
}
```

Champ **privé** : seuls `resource` et `locked` le lisent. Un agent qui pourrait
le lire pourrait le contourner.

> C'est l'idiome que `EngineContext` s'impose déjà pour le `World` — *« reach it
> through `world_ref`/`world_mut`, never by matching directly »*. On l'étend, on
> ne l'invente pas.

### Ce que `AgentAccess` devient

Son sens rétrécit au `World`, et rien d'autre :

| Variante | Sens après |
|---|---|
| `Exclusive` | `&mut World` — vague solitaire |
| `SharedWorld` | `&World` en lecture |
| `Isolated` | ne touche pas le `World` |

**Et la règle « au plus un `SharedWorld` par vague » tombe.** Elle existait
parce que ces agents « peuvent écrire des ressources partagées » sans qu'on
sache lesquelles. Déclarées, plusieurs `SharedWorld` partagent une vague si leur
contention est disjointe. Un seul reste inline (il tient le `&World`) ; les
autres passent au pool comme les `Isolated`.

### L'exemption, nommée

`AgentFrameStatusMap` est écrit par l'ordonnanceur et lu par les agents dans
`report_status`. Il est **engine-owned** : hors du contrat, et dit comme tel
dans la doc du trait plutôt que laissé à deviner. C'est la seule exemption ;
toute autre serait une brèche.

---

## Migration

Sept sites, cinq agents :

| Agent | Ce qu'il atteint |
|---|---|
| `RenderAgent` | `AssetStore`, `Arc<FrameContext>` |
| `OverlayAgent` | `AssetStore`, `Arc<FrameContext>` |
| `ShadowAgent` | `AssetStore` |
| `SkyboxAgent` | `Arc<FrameContext>` |
| `UiAgent` | `Arc<FrameContext>` |

Tous en lecture — d'où `reads`. `ScriptingAgent` est le premier à déclarer un
`writes` (il **draine** `PendingScriptReloads` et `PendingScriptEvents`), et
reste `Isolated`.

---

## Ce que les tests doivent prouver

Écrits **avant** le code : ils *sont* la sémantique.

1. **Un accès non déclaré est refusé** — `resource::<T>()` rend `None` et
   journalise, sur un agent dont la `Contention` ne mentionne pas `T`.
2. **`locked` exige `writes`** — déclarer `T` en lecture seule ne suffit pas.
3. **La collision est détectée et nommée** — deux agents d'une même vague qui
   verrouillent la même ressource ; puis un écrivain et un lecteur.
4. **Deux lecteurs coexistent** — `reads ∩ reads` ne déclenche rien.
5. **La composition des vagues**, et c'est le test qui compte le plus :
   `RenderAgent` et `UiAgent` doivent se retrouver **dans la même vague
   concurrente**.

### Le mode de panne que seul le test 5 attrape

Une déclaration **trop large** échoue en silence, *dans le sens de la lenteur* :
tout se sérialise, rien ne casse, aucun test ne rougit, le moteur ralentit. Une
vérification qui n'assertent que la détection de collision ne le voit jamais —
elle est même satisfaite par un agent qui déclare tout.

D'où l'assertion sur la **composition** des vagues, pas seulement sur les
conflits. C'est le seul énoncé qu'une sur-déclaration fait rougir.

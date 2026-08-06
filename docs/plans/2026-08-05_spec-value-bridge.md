# Spec — le pont de valeurs d'Ergon

> Contrat d'implémentation du chantier 1 de
> [la consolidation](./2026-08-05_ergon-consolidation.md).
> **Cette table est la spec.** Son exhaustivité *est* la correction : le bug
> qu'elle supprime vient d'une case laissée vide dans une traduction et remplie
> dans une autre.

## Le problème, en une phrase

Quatre traductions du même univers de valeurs, écrites à la main, à des moments
différents, par le même auteur, et déjà divergentes :

| Emplacement | Fonction | Ce qu'elle couvre |
|---|---|---|
| `khora-script/src/dispatch.rs` | `to_register` | Unit, Bool, Int, Float, Entity |
| `khora-script/src/native/events.rs` | `carried` | idem **+ Vec3, Str** |
| `khora-lanes/src/script_lane/persistence.rs` | `to_persisted` / `to_script_value` | idem + Vec3, Str |
| `khora-data/src/ecs/systems/script_commands/json.rs` | `to_json` | tout |

`Raise` produit un `ScriptValue::Vec3` (via `carried`) ; la livraison le refuse
(via `to_register`) ; la lane traite le refus comme une faute ; **le behavior
cible est désactivé définitivement**. Idem pour une `string`.

---

## Les trois univers

Ils ne sont pas redondants — chacun répond à une contrainte différente, et c'est
pour ça qu'on ne les fusionne pas en un seul type.

| Univers | Où | Contrainte qui le façonne |
|---|---|---|
| `Value` | `khora-script/src/vm/value.rs` | Tient dans un registre, `Copy`, gelé et dégelé à chaque suspension. Une chaîne y est une **référence** (`StrRef`), pas des octets. |
| `Persisted` | `khora-script/src/arena/persistent.rs` | Survit à la frame et à une sauvegarde. Une chaîne y est **possédée** — une référence d'arène serait un pendouillant garanti. |
| `ScriptValue` | `khora-core/src/script/value.rs` | Vocabulaire de frontière : sérialisable, lisible par l'éditeur, indépendant de la VM. C'est le seul que `khora-core` connaît. |

`Persisted` enveloppe `Value` (`Scalar`) ou possède (`Owned(Object)`), donc la
paire `Value`/`Persisted` est une seule décision : **inline** ou **possédé**.

---

## La table

Une ligne par variante de `ScriptValue`. `—` signifie « pas de forme dans cet
univers », et la colonne *Comportement* dit ce qui se passe alors — jamais un
silence.

### Régulières — ce que la macro génère

| `ScriptValue` | `Value` | `Persisted` | `Json` | Type Ergon |
|---|---|---|---|---|
| `Bool(bool)` | `Bool` | `Scalar` | `Bool` | `bool` |
| `Int(i64)` | `Int` | `Scalar` | `Number` | `int` |
| `Float(f32)` | `Float` | `Scalar` | `Number`¹ | `float` |
| `Entity(EntityId)` | `Entity` | `Scalar` | objet² | `Entity` |
| `Vec2(Vec2)` | `Vec2` ⟵ **nouveau** | `Scalar` | objet² | `Vec2` |
| `Vec3(Vec3)` | `Vec3` | `Scalar` | objet² | `Vec3` |
| `Vec4(Vec4)` | `Vec4` ⟵ **nouveau** | `Scalar` | objet² | `Vec4` |
| `Quat(Quaternion)` | `Quat` ⟵ **nouveau** | `Scalar` | objet² | `Quat` |
| `Color(LinearRgba)` | `Color` ⟵ **nouveau** | `Scalar` | objet² | `Color` |

¹ `to_json` **refuse** `NaN` et les infinis : JSON n'a pas de forme pour eux, et
les convertir donnerait `null`, qui se relit comme un champ absent. Le refus
nomme le champ.
² Sérialisé par `serde`, donc la forme JSON est par construction celle que le
miroir du composant attend — l'écrire à la main serait une seconde définition
libre de dériver.

### Irrégulières — bras explicites, jamais générés

| `ScriptValue` | `Value` | `Persisted` | Comportement |
|---|---|---|---|
| `Unit` | `Unit` | `Scalar(Unit)` | **Asymétrique, exprès.** Vers la VM : `Unit`. Depuis un store : `Unit` veut dire *slot jamais écrit*, donc `snapshot_from_store` rend `None` et laisse l'initialiseur produire le défaut déclaré. Une scène qui enregistrerait « ce champ n'a pas de valeur » masquerait ce défaut. |
| `Str(String)` | `Str(StrRef)` | `Owned(Object::Str)` | Trois formes distinctes. Vers la VM : la chaîne doit être **allouée dans l'arène** (`StrRef::Arena`) — une valeur de frontière n'a pas d'index dans la table de constantes du programme. Depuis la VM : **résolue et copiée** (`NativeContext::string`), parce que l'arène est libérée en fin de frame. Depuis un store : seule la forme `Owned` est lisible ; un `Scalar(Str(_))` persisté serait déjà pendouillant. |
| `Array(Vec<ScriptValue>)` | — | `Owned(Object::Array)`³ | Refusé vers un registre. `Object::Array` tient des `Value`, pas des `ScriptValue` : une conversion élément par élément est possible mais aucun appelant n'en a besoin aujourd'hui. **Refus journalisé**, pas silence. |
| `Struct(Vec<(String, …)>)` | — | — | Existe pour l'écriture partielle d'un composant (`health.current = 50`), qui vit dans `to_json` + `merge`. Aucune forme VM, aucune forme persistée. |
| — | `Null` | `Scalar(Null)` | **N'a pas de forme de frontière et ne doit pas en avoir.** `Null` est l'optionnel absent, et deux usages internes s'y ajoutent : un `after` épuisé, et un slot de décompte non armé. Un événement ne peut pas porter `null` (le handler déclare `int amount`, pas `int? amount`) : `Raise` le refuse en le nommant. |

³ Réservé : rien ne l'écrit aujourd'hui.

---

## La forme du code

### La table, dans `khora-core/src/script/table.rs`

Une X-macro : elle ne définit rien, elle **rappelle** un callback avec le
tableau. Chaque crate consommateur fournit son propre callback et génère la
forme dont il a besoin.

```rust
#[macro_export]
macro_rules! script_value_table {
    ($callback:path) => { $callback! {
        //  variante  type Rust     type Ergon
        Bool    : bool            , Bool            ;
        Int     : i64             , Int             ;
        Float   : f32             , Float           ;
        Entity  : EntityId        , Entity          ;
        Vec2    : Vec2            , Engine("Vec2")  ;
        Vec3    : Vec3            , Engine("Vec3")  ;
        Vec4    : Vec4            , Engine("Vec4")  ;
        Quat    : Quaternion      , Engine("Quat")  ;
        Color   : LinearRgba      , Engine("Color") ;
    }};
}
```

Les irrégulières n'y sont **pas**. Les mettre avec un drapeau « spécial »
reviendrait à écrire la logique dans la macro : elles restent des bras à la
main, donc visibles *comme* exceptions.

### Le pont, dans `khora-script/src/bridge.rs`

Le seul module qui connaisse les trois univers. Il consomme la table et expose :

```rust
/// Vers un registre. `Err` nomme ce qui ne peut pas voyager.
pub fn to_register(value: &ScriptValue, arena: &mut Arena) -> Result<Value, Unrepresentable>;

/// Depuis un registre. Résout et copie les chaînes.
pub fn from_register(value: Value, strings: &[String], arena: &Arena) -> Result<ScriptValue, Unrepresentable>;

/// Vers le stockage persistant.
pub fn to_persisted(value: &ScriptValue) -> Result<Persisted, Unrepresentable>;

/// Depuis le stockage persistant. `Ok(None)` = slot jamais écrit.
pub fn from_persisted(value: &Persisted) -> Result<Option<ScriptValue>, Unrepresentable>;
```

`Unrepresentable { value_kind: &'static str, direction: &'static str }` : un
seul type d'échec, qui nomme la valeur et le sens. Les appelants le traduisent
dans leur propre vocabulaire (`NotDelivered::UnsupportedArgument`,
`NativeError`, un `log::warn`).

> **`to_register` prend l'arène** — c'est le changement d'API qui rend le bug
> impossible. Une chaîne de frontière ne peut devenir un `Value` qu'en étant
> allouée quelque part, et l'ancien `to_register` n'avait pas où : il **devait**
> refuser `Str`. Lui donner l'arène supprime la raison du refus au lieu de la
> contourner.

### Ce qui disparaît

`dispatch::to_register`, `native::events::carried`,
`persistence::to_persisted`, `persistence::to_script_value`. Les quatre
deviennent des appels. `json::to_json` reste où il est (il ne connaît que
`ScriptValue`) mais consomme la table pour ses bras réguliers.

---

## Ce que les tests doivent prouver

Écrits **avant** le code. Le premier échoue aujourd'hui.

1. **Le bug.** Un script fait `Raise(e, "Hit", Vec3(1,0,0))` ; la frame suivante,
   le handler `on Hit(Vec3 where)` s'exécute et lit le vecteur. Idem avec une
   `string`.
2. **Aller-retour complet, une assertion par ligne de la table** :
   `ScriptValue` → `Value` → `ScriptValue` et `ScriptValue` → `Persisted` →
   `ScriptValue` rendent la valeur de départ.
3. **Les irrégulières font ce que la table dit** : `Unit` depuis un store rend
   `None` ; `Array` vers un registre est refusé **en le nommant** ; une chaîne
   qui traverse la VM est copiée, pas référencée.
4. **`Null` ne franchit pas la frontière** : `Raise` avec `null` échoue avec un
   message qui explique pourquoi.
5. **Exhaustivité imposée par le compilateur** : le `match` du pont n'a pas de
   bras `_`. Ajouter une variante à `ScriptValue` ou à `Value` doit **casser la
   compilation** du pont, et de lui seul.

Le point 5 est le vrai livrable. Le reste vérifie l'état d'aujourd'hui ; le
point 5 empêche demain.

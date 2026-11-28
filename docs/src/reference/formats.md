# File formats

The on-disk formats Khora reads and writes: the `.kscene` scene file, the
`.kprefab` prefab, the game save, the `.pack` asset archive, and the `.kmat`
material file. This page is structure and facts —
for *why* the formats are shaped this way see
[Serialization](../concepts/serialization.md) and [Assets](../concepts/assets.md);
to *save and load* a scene see
[Save and load scenes](../how-to/save-and-load-scenes.md).

## `.kscene` — scene file

A `.kscene` file is a fixed-size header followed by a single payload. The payload
is a **scene record**; the header names the encoding it was written in, so a file
is read back without knowing in advance how it was saved.

### Header layout

The header is a fixed `SceneHeader` (defined in
`crates/khora-core/src/scene/format.rs`). Its size is **49 bytes**
(`SceneHeader::SIZE = 8 + 1 + 32 + 8`):

| Field | Type | Bytes | Notes |
|---|---|---|---|
| `magic_bytes` | `[u8; 8]` | 8 | `"KHORASCN"` for a scene or prefab (`HEADER_MAGIC_BYTES`), `"KHORASAV"` for a game save (`SAVE_MAGIC_BYTES`). |
| `format_version` | `u8` | 1 | `SCENE_FORMAT_VERSION`, currently `2`. |
| `encoding_id` | `[u8; 32]` | 32 | Null-padded UTF-8 encoding ID, e.g. `"KH_COMPACT_V2"`. |
| `payload_length` | `u64` | 8 | Length of the payload that follows, in bytes (little-endian). |

The payload immediately follows the header. `SceneFile::to_bytes` /
`SceneFile::from_bytes` serialize and parse the whole file; the header is written
by direct byte manipulation (not serde) because it is fixed-layout.
`from_bytes` returns `SceneFileError::InvalidMagicBytes` or
`SceneFileError::TooShort` on a malformed file.

Reading the payload can fail with a `SceneFileReadError`:

| Variant | When |
|---|---|
| `OldFormat(version)` | written in a format this engine no longer reads |
| `NewerFormat(version)` | written by a newer engine |
| `UnknownEncoding(id)` | the header names an encoding this build does not have |
| `Encoding(error)` | the payload could not be decoded |
| `NotAScene` | a scene was expected and the file is a game save |
| `NotASave` | a game save was expected and the file is a scene |

### The scene record

What every encoding holds is the same `SceneRecord`
(`crates/khora-data/src/scene/scene_record.rs`), shaped like the world's pages:

- `entities` — the persistent id of every saved entity, in order.
- `pages` — one per component signature: the component **names**, the ids of
  the rows, and one column of values per component, row-aligned.
- `instances` — the [prefab instances](../concepts/prefabs.md) the record keeps
  collapsed: each an `InstanceRecord { root, prefab, delta }` — the root's
  persistent id, the prefab's asset UUID, and the instance's differences from
  the prefab as a game save records them. Text and MessagePack read the key as
  optional: a record without instances may leave it out.

Components are saved **by name** — the name their `Component` derive registers —
and their values field by field, by name. Entity references are saved as
persistent ids, asset references as asset UUIDs. Only components whose
provenance is authored or tool-authored are saved; derived and runtime state is
rebuilt on load.

Reading a record adapts it to today's code and **reports** each adaptation
(`LoadReport`): a field missing from the save takes its default, a field the code
no longer has is dropped, a field or component saved under a name listed in its
`formerly` is renamed, a number is read at another width losslessly, a reference
to an entity the save does not hold is cut. A component name nothing registers is an
**error** — unless it is declared retired, in which case it is skipped and
reported. A component the engine derives or keeps while running is skipped and
reported too, and rebuilt: a save never holds one, and a recorded copy could only
disagree with what it comes from. Text encodings write a float JSON has no number
for as a marker (see [Text and MessagePack values](#text-and-messagepack-values)).
Every encoding refuses a value nested deeper than 128 levels. Loading is atomic: the record is validated and
staged before the world changes, so a refused file leaves the world as it was.

### Encodings and goals

The payload encoding is chosen by a `SerializationGoal`, **not** by file
extension. There are **three** record encodings and one **snapshot**, each
identified by a versioned ID:

| Encoding | `encoding_id` | Payload | Character |
|---|---|---|---|
| Compact | `KH_COMPACT_V2` | Binary, page-shaped, column-major; component and field names stored once in symbol and shape tables | Small and fast; the editor's working format. |
| Text | `KH_TEXT_V2` | Pretty-printed JSON | Human-readable, diffable. |
| MessagePack | `KH_MSGPACK_V2` | MessagePack, fields by name | Portable; readable by any MessagePack library. |
| Snapshot | `KH_SNAPSHOT_V1` | Positional, no names; every component listed with its schema fingerprint | The fastest load, readable only by the schema that wrote it. |

`SerializationGoal` (in `khora-core::scene`) has **six** variants. The goal →
encoding mapping is made in `SerializationService::save_world`:

| `SerializationGoal` | Encoding |
|---|---|
| `HumanReadableDebug` | Text (`KH_TEXT_V2`) |
| `LongTermStability` | Text (`KH_TEXT_V2`) |
| `SmallestFileSize` | Compact (`KH_COMPACT_V2`) |
| `EditorInterchange` | Compact (`KH_COMPACT_V2`) |
| `FastestLoad` | Snapshot (`KH_SNAPSHOT_V1`) |
| `PortableBinary` | MessagePack (`KH_MSGPACK_V2`) |

### Text and MessagePack values

The text and MessagePack encodings write each value as a tree a reader can walk
without knowing its type. Most of it is the value's own serde form: a struct is a
map of its fields by name, a unit struct `{}`, a unit enum variant its bare name,
any other variant `{"Variant": payload}`. Three kinds of value carry a
**marker**, a map of exactly one entry whose key starts with `$` (a map of more
entries is plain data):

| Value | Written | Read back from |
|---|---|---|
| an entity reference | `{"$entity": 1234567}`, the persistent id's 64 bits | an unsigned or non-negative integer |
| an entity outside the record | `{"$entity": null}` | null, or a unit |
| an asset reference | `{"$asset": "6f1c…-uuid"}` | the uuid's text |
| a float JSON has no number for | `{"$float": "inf"}`, `"-inf"`, `"nan"` | text only: MessagePack writes the float itself |

**Decimal width.** A number written as decimal text — any float in the text
encoding — is the value its digits spell, whatever width wrote it. Read into an
`f32` field, it is parsed as an `f32`: `0.1` is the `f32` nearest 0.1, not the
`f64` 0.1 narrowed. It is refused only when no `f32` holds it, and its reading is
not reported. A binary float is its bits: an `f64` read into an `f32` must be
exact, and an integer read from a float must be whole, finite and below 2⁶⁴;
those reads are reported `Widened`. The compact encoding keeps a text decimal as
such (its own tag, the `f64` after it), so a text file converted to compact
reads the same.

### Snapshot layout

A snapshot holds values **by position**: no field names, no record tree, nothing to
match on load. What makes that safe is the **component table** at its head: every
component the snapshot holds, by name, with the **fingerprint** of its schema — a
hash of its serde format traced in full (struct and field names, their order, their
types, every enum variant, nested types). A snapshot is read only if every
fingerprint equals the running build's; otherwise the whole file is refused, the
world unchanged, naming the component. A snapshot is a cache of a stable save —
the editor's Play/Stop state, a shipped build's data — never the only copy of a
scene.

The fingerprint is only as good as the trace behind it. A type is traced by driving
its own `Deserialize` with placeholder values, retried until the type accepts one,
and a type met again inside itself is read once more for its smallest value so the
trace goes on past it. A component whose schema still cannot be traced in full (a
field that refuses every placeholder) cannot be guarded: it is never written to a
snapshot — `FastestLoad` saves such a world as a compact record instead, with a
warning — and a snapshot naming it is refused.

<div class="kp-figure-frame">

{{#include ../images/persistence/snapshot-vs-record.svg}}

</div>

A component a script declares has no Rust schema to trace. Its table entry carries
its declaration instead — its domain and each field's name, kind and default — and
its rows are read back onto the running world's declaration **by name**, as a record
would be: a hot reload during Play may have given it new fields, and Stop must still
restore.

Payload: `version: u8` (`2`; layout `1`, Rust components only, still reads); the
component table — a count, then each name (length, UTF-8), an entry kind and what it
holds: `0` and the fingerprint (`u64` LE) for a Rust component, `1`, the domain's
index and the fields (a count, then each name, kind name, Ergon type — its signature,
empty when unknown — and positional default) for a declared one; the entities' persistent ids; the pages —
each its components by table index, its rows, then its values column after column,
each value its length and its positional bytes. A positional value is fields in
order: varints for counts and integers (signed ones zig-zagged), floats at their
width, `0`/`1` for a `bool` or an option's presence, a variant index then its
payload, a map's entries sorted by their bytes (so the same world always writes the
same snapshot). Entity references are still written as persistent ids and asset
references as UUIDs, so a snapshot keeps identities exactly as a record does.

The positional form, field by field:

| Value | Bytes |
|---|---|
| `bool`, an option's presence | one byte, `0` or `1`, then the value when present |
| `u8` | the byte itself |
| other integers | a LEB128 varint; signed ones zig-zagged first |
| `f32`, `f64` | their bits, little-endian |
| a string, bytes | a varint length, then the bytes |
| an entity reference | `0` for an entity outside the record, else `1` and the persistent id as 8 bytes LE |
| a sequence, a map | a varint count, then the elements; a map's entries sorted by their key's bytes |

A count may exceed the bytes left by at most 256 elements, written or read, so a
forged count cannot make the reader allocate past the file. A sequence whose
length is not known when it is written is refused. Nesting stops at 128 levels on
write as on read. A value must use its bytes exactly: one byte left over refuses
it. In the table, a declared component's domain is one byte, its index, and a
field's Ergon type is empty when it is not known.

### Schema text

A fingerprint is the FNV-1a 64-bit hash of a **schema text**, written with no
spaces:

| Type | Text |
|---|---|
| primitives | `bool`, `i8` … `u64`, `f32`, `f64`, `char`, `string`, `bytes`, `unit` |
| option, sequence, map, tuple | `option<T>`, `seq<T>`, `map<K,V>`, `tuple(T,U)` |
| structs | `struct Name`, `struct Name(T,U)`, `struct Name{a:T,b:U}` |
| enums | `enum Name{A,B(T),C(T,U),D{x:T}}` |
| a type met again inside itself | its name alone |
| a type or a variant's payload the trace could not reach | `?` |

A trace that did not reach every value ends its text with `!incomplete`: its
component cannot be guarded, as above.

A declared field's Ergon type is written as its **signature**: `bool`, `int`,
`float`, `Duration`, `Angle`, `string`, `Entity`; an asset kind (`Prefab`,
`Sound`, `Material`, `Mesh`, `Texture`, `Scene`, `Font`, or a game's own); an engine type (`Vec2`, `Vec3`,
`Vec4`, `Quat`, `Color`); a struct as `Name{field:T,…}`; each followed by any
number of `?` (optional) and `[]` (array) suffixes — `Mesh?[]` is an array of
optional meshes. Records, compact declarations and snapshot tables all carry this
text. A field's **kind**, its storage, is another name beside it: `Bool`, `Int`,
`Float`, `Vec2`, `Vec3`, `Vec4`, `Quat`, `Color`, `Entity` or `Value`.

On 10 000 entities (a transform and a name each), a snapshot loads in about
9 ms where the compact record takes about 79 ms
(`cargo bench -p khora-data --bench data -- "Scene load"`).

### Compact layout

The compact payload starts with its layout version — `2`, or `3` — and writes every
name once. A table of **symbols** (component, struct, field, enum and variant
names) and a table of **shapes** (a struct's name and field names, or an enum's
name and a variant's) come first; every value after them refers to its shape or
name by index. The entities and the pages follow, each page column by column —
the order CRPECS stores them in — and last the prefab instances: each its root
(`u64` LE), its prefab (16-byte UUID), then its delta behind its length, in the
game save layout below. A record holding components a script declares is written as
layout `3`, which ends with their declarations — a count, then each as one value:
its name, domain and fields (name, kind, Ergon type, default). Any other record is still written
as layout `2`, byte for byte. Layout `1`, which has no instances, still reads.

<div class="kp-figure-frame">

{{#include ../images/persistence/compact-layout.svg}}

</div>

A value is a one-byte tag followed by its content: numbers as LEB128 varints
(signed ones zig-zagged) or little-endian floats, a struct as its shape index then
its fields in the shape's order, a variant as its shape index, a payload kind, then
the payload. Reading checks every count against the bytes left before allocating,
nests at most 128 levels, keeps its own stack of open values rather than recursing,
and refuses a file whose values would copy more than 64 bytes of names per byte of
file.

On load, the `encoding_id` in the header picks the decoder — the goal is
irrelevant at load time. Every encoding decodes to the same record, so the rules
above hold whichever one wrote the file.

> Choosing a *goal* is a developer decision; choosing an *encoding* is an engine
> decision.

## `.kprefab` — prefab

A prefab is a scene file — the same header, magic `KHORASCN` — whose record holds
one subtree: the entity it was made from and everything under it. The editor
writes it in the compact encoding. A prefab may itself hold instances of other
prefabs, but never of itself, at any depth. See [Prefabs](../concepts/prefabs.md).

## Game save

A game save is a file with the same header and magic `KHORASAV`. Its payload is a
`SaveRecord` (`crates/khora-data/src/scene/save/mod.rs`): how the running world
differs from the scene it was taken against. See
[Scenes and game saves](../concepts/saves.md).

| Field | Holds |
|---|---|
| `base` | the scene's asset UUID |
| `order` | every entity of the saved world, tree by tree, siblings in order |
| `destroyed` | ids of the scene's entities the game destroyed |
| `created` | ids of the entities the game made |
| `removed` | per scene entity, the names of the components the game removed |
| `changes` | a scene record: every entity that differs, with the components that differ, whole |
| `before` | a scene record: the scene's values, when saved, of the components in `changes` |

In the compact encoding: `base` (16 bytes), then `order`, `destroyed` and
`created` — each a varint count and `u64` LE ids — then `removed` — a count, and
per entry an id, a count and length-prefixed names — then `changes` as a compact
record behind its length, then `before` as a compact record. Text and
MessagePack write the same fields by name. A save is never written as a
snapshot: asked for `FastestLoad`, it is written compactly.

## `.pack` — asset archive

In release builds, a project's assets are bundled by
`khora_io::asset::PackBuilder` into three files, staged under
`<project>/dist/<target>/`:

- `data.pack` — a header, then every asset's bytes and every container's
  external file's bytes, each compressed or not;
- `index.bin` — the asset entries, mapping each asset's UUID to its place inside
  `data.pack`, then the external-files table, mapping each external file's path
  to its place;
- `manifest.bin`, when the build signs the pack (Build Game always does) — the
  signed manifest: the size and BLAKE3 hash of the uncompressed bytes of each
  asset, by UUID, and of each external file, by path, signed over the hash of
  `index.bin`. A shipped game refuses a pack without one; see
  [Trust: signed packs](../concepts/trust.md#signed-packs).

The split keeps the loader's I/O simple and lets a tool inspect each file on its
own.

### `data.pack` layout

```
data.pack
┌──────────────────────────────────────────────────┐  offset 0
│ Header (24 bytes, little-endian)                  │
│   ─ magic:          "KHORAPK\0"  (8 bytes)        │
│   ─ format_version: u32          (4 bytes) = 3    │
│   ─ asset_count:    u32          (4 bytes)        │
│   ─ flags:          u32          (4 bytes)        │
│   ─ reserved:       u32          (4 bytes) = 0    │
├──────────────────────────────────────────────────┤  offset 24
│ asset 0 bytes                                     │
│ asset 1 bytes                                     │
│ …                                                 │
│ asset N-1 bytes                                   │
│ external file 0 bytes                             │
│ …                                                 │
└──────────────────────────────────────────────────┘
```

The header lets `PackLoader::new` fail fast on a wrong or renamed file (bad
magic) or a pack from another engine version (`format_version` must equal
`PACK_FORMAT_VERSION`). `asset_count` holds `index.bin`'s asset entry count,
sub-assets included, external files not. `flags` is for diagnostics: bit 0 says some entry was written
LZ4-compressed, bit 1 that a `manifest.bin` accompanies the pack. Every byte
after the header is payload, assets then external files, with no per-entry
framing. Offsets recorded in
`index.bin` are **relative to the asset region** (the first asset is at 0); the
loader adds the header size when seeking.

Each entry is compressed on its own, or not: its `AssetSource::Packed` in
`index.bin` gives its on-disk `size`, its `uncompressed_size` and its
`compression`. A loaded entry is decompressed, then checked against the
manifest's size and hash.

The constants are public in `khora_io::asset`: `PACK_MAGIC`,
`PACK_FORMAT_VERSION`, `PACK_HEADER_SIZE` (all three re-exported through
`khora-sdk`), `PACK_FLAG_LZ4` and `PACK_FLAG_MANIFEST`.

### `index.bin` layout

`bincode(Vec<AssetMetadata>)` then `bincode(Vec<PackedFile>)`, both encoded with
`bincode::config::standard()` (`AssetIndex::encode`), and nothing after them. A
loose project's index (built in memory by the editor and a development run) has
the same shape, its external-files table empty; an index missing its table or
followed by other bytes is refused. Each `AssetMetadata` carries:

| Field | Meaning |
|---|---|
| `uuid` | `AssetUUID` — the asset's stable identity: `AssetUUID::new_v5(forward_slash_rel_path)` by default, or the frozen value the [identity registry](#asset-identity-registry) holds for that path. |
| `kind` | `AssetKind` — the asset's kind, encoded as its registered name (`"texture"`, `"mesh"`, `"material"`, `"sound"`, `"script"`, …); a name no registration declares is refused on read. A file of no kind is not indexed. |
| `dependencies` | `Vec<AssetUUID>` — assets this one references (deduplicated, sorted). |
| `variants` | `HashMap<String, AssetSource>` — the `"default"` variant points into `data.pack` as `Packed { offset, size, uncompressed_size, compression }`; a sub-asset keeps its `Sub` variant, extracted from its container. |
| `tags` | `Vec<String>`. |

The **external-files table** lists each file a packed container reads beside
itself that is no asset (a glTF's `.bin`): no kind, no UUID. Each `PackedFile`
carries its forward-slash `path` under `assets/` and its place, `offset`, `size`,
`uncompressed_size` and `compression`, as an asset entry's `Packed` source does.
The container's resolver finds it by path and reads it under the container's
size cap; a file a container reads that is an asset (a glTF's texture) is packed
once, as the asset it is. The pack builder reads every file through the
[guard](../concepts/trust.md#the-guard), reporting to the refusals it is given
(`PackBuilder::with_refusals`; Build Game gives the run's): an external file the
guard refuses, its path leaving `assets/` by `..` or through a link, or its size
over its container's cap, is not packed, its refusal reported and logged, and
the build goes on.

### Determinism

`IndexBuilder` sorts asset paths lexicographically (forward-slash relative path)
before `PackBuilder` streams them, so two consecutive packs of the same `assets/`
directory are byte-identical. Each asset's UUID is resolved the same way in both
modes: the frozen entry from the [identity registry](#asset-identity-registry)
if one exists, otherwise the default `AssetUUID::new_v5(forward_slash_rel_path)`.
Because dev (`IndexBuilder::with_registry`) and release (`PackBuilder`, which loads
the same registry) resolve through it identically, a UUID is **the same in dev mode**
(loose files + in-memory index) **and release mode** (packed file + on-disk
`index.bin`) by construction. Game code carrying an `AssetHandle<T>` works in either
mode unchanged.

## Asset identity registry

An asset's `AssetUUID` must survive a rename or move so that references stored as
raw UUID bytes — an `AssetRef::Asset` of any kind, `.kmat` texture slots — never
break. The `<project_root>/.khora/asset-registry.ron` file decouples identity from
path: it freezes a stable UUID for a relative path (`AssetIdRegistry` in
`crates/khora-io/src/asset/id_registry/mod.rs`).

**Lazy freeze.** An asset that has never been renamed has **no** registry entry and
keeps its `AssetUUID::new_v5(rel_path)` default, so pre-registry projects and tests
are unaffected. The first time an asset is renamed or moved in the editor, its
*current* UUID is frozen into the registry — so it keeps that UUID forever,
regardless of any future path change. A rename therefore rewrites nothing: scenes,
prefabs, and `.kmat` files on disk and the open scene all keep resolving through the
unchanged UUID. Renaming or moving a folder freezes and moves the identity of
every file inside it at once (`AssetIdRegistry::rename_folder`); the entry of a
file of that folder in the recycle bin stays at its path, for a restore there.
Where file names ignore case, a rename in case only (`Tex` to `tex`) renames the
entry itself, through a temporary name beside it (`khora_io::fs::rename_entry`), and
moves its identities to the new spelling like any rename. A folder renamed so moves
the entries its trashed files left too (`AssetIdRegistry::rename_folder_in_case`):
both spellings name one folder, so a file restored from the recycle bin lands under
the new one, and gets its identity back there. Two names are one entry only when
they sit in one folder, are equal but for case, and are one entry on disk
(`khora_io::fs::is_same_entry`): two hard links to a file are two entries.

**A rename cut short.** The temporary name of a rename in case only
(`<name>.<pid>.<n>.aside.tmp`) is never walked: the index, the watcher and the asset
browser leave out every folder named as a temporary (`khora_io::fs::is_temporary`),
whatever it holds. An entry a crash left under it is put back when the project opens
(`khora_io::fs::reclaim_asides`): under its name when nothing holds it, otherwise left
where it is, never removed, and listed in the Problems panel.

**Paths the editor accepts.** Every file operation names its file by a forward-slash
path under `assets/`, and the editor never renames a path to make it fit: a name
it would have to change is refused, naming the part refused, and nothing is read or
written. The rule depends on the path's role (`crates/khora-editor/src/project_vfs/asset_path.rs`):

- **An existing path** — the source of a rename, a move or a trash, a file written
  over — is checked to stay in `assets/` (`check_existing`): no empty part (so no
  absolute path), no `.` or `..`, and no link leading out. Its name is held to no
  rule about names, so a file made on another system under a name Windows refuses
  can still be renamed, moved or trashed. Each part that exists is instead asked of
  the system: its folder must list it under that spelling (or in another case, where
  file names ignore it, the spelling on disk kept), and opening it by that name must
  reach that entry, not a device nor the entry the system trims the name to
  (`project_vfs/reached.rs`). So whatever a Windows version makes of `con.png` or of
  a trailing dot, the editor follows: a `con.png` this Windows made and lists is
  renamed like any file, and `v1.` beside a folder `v1` is refused, "Windows reaches
  another entry under 'v1.'".
- **A new name** — a file or folder made, a duplicate, the destination of a rename,
  a new file saved — must be a name every system keeps as typed (`check_new_name`), so
  a project moves between systems unchanged. Besides the above, it holds no `\`
  (another folder on Windows), no `:` (a drive or a stream), no control character and
  none of `<>"|?*` (refused by Windows); it does not end with a dot or a space (which
  Windows drops, so the file made would not be the one named, and could be one beside
  it); and it is no device name Windows keeps in every folder (`CON`, `PRN`, `AUX`,
  `NUL`, `COM0`–`COM9`, `LPT0`–`LPT9`, in any case and with any extension: `nul.kscene`
  is the null device, and a write there is lost). The asset browser's rename boxes
  check the name typed by this rule before asking, and every name the editor makes
  for you is one it accepts: a new script's (`aux` gives `Aux2.erg`, as a taken name
  does), a prefab's or a material's named after an entity (`con` gives `con_`), a
  built game's binary. A duplicate whose ` copy` name is no new name is refused.

The folders of a path are filed as spelled on disk, whatever case the path gives them,
so the registry and the index name the file alike. A file picked in a dialog
(`ProjectVfs::in_project`) is *outside* the project, *inside* it at a path, or
*refused*: a path under the assets whose new part is no new name, whose existing part,
as typed or as resolved, the system does not reach by its name, or that leads out of
`assets/` through a link. A folder `aux/` the system made and reaches is inside.
Whether a path is inside is decided on its resolved form, links followed as far as it
exists (`khora_io::fs::resolved_relative_to`, the rule the
[guard](../concepts/trust.md#the-guard) refuses an escape by), and nothing is written
through such a link.

**Derived UUIDs.** The default of a renamed asset's old path stays the renamed
asset's. A new file made at that path gets a UUID of its own instead, derived from
its path, a NUL and a count: no file name holds a NUL, so a derived UUID is never
another path's default. The editor freezes it (`AssetIdRegistry::freeze_derived`),
so the file keeps it once the renamed asset is gone. A dependency naming a path —
a script's import — resolves through the registry too, so it names the frozen
UUID of a renamed asset and the derived UUID of a new file at an old path, as the
index does (see [Assets — dependency tracking](../concepts/assets.md#dependency-tracking)).

**Trash and new files.** Sending an asset to the recycle bin leaves the registry as it
is: the asset keeps its frozen identity, so a file restored at its path gets it back,
and every reference to it resolves again. That entry is kept, stale, until a new file
takes the path. A new file made at a path where no file is — a script or a file made
in the asset browser, a duplicate, a scene, material or prefab saved there — is
another asset: the editor drops the entry first and saves the registry, so the new
file never takes the trashed file's identity (`AssetIdRegistry::resolve_unfrozen`
gives the id it gets, before it is written). A file written over keeps its identity:
it is the same asset, even named in another case where file names ignore case
(`Steel.kmat` over `steel.kmat`). Every editor write lands on the file found under the
name it has. A write whose id the editor needs is reserved first
(`ProjectVfs::reserve_write`): the registry's file followed, and the id the file will
hold given, which a prefab writes into itself and a material saved from an entity is
assigned by; the write then lands on that file with that id, or is refused when the
project changed in between. A write that needs no id, a scene saved, is
`ProjectVfs::write_asset`. A file brought in from outside the editor (a copy in the file
explorer, a checkout) at such a path takes the entry it finds there.

**Format.** RON, one `(uuid, path)` entry per asset, **sorted by UUID**. Because the
UUID is immutable, renaming an asset, or making a file where a trashed one was, each touch a single line,
so two branches that rename *different* assets produce non-overlapping diffs that git
3-way-merges cleanly (a real conflict arises only when the same asset is renamed on
both branches). The file is written **atomically** (temp file + rename) so a crash
mid-write cannot corrupt it.

**Placement.** The registry lives at the **project root**, a sibling of `assets/`, so
it is never scanned, watched, or packed — the scanner, the filesystem watcher, and
the packer are all rooted at `assets/`. The editor (`ProjectVfs`) is the **only**
writer; the read side (`IndexBuilder::with_registry`, the runtime, and `PackBuilder`)
resolves through it, which is what makes dev and release agree on identity.

**Reading.** `AssetIdRegistry::load` reads the file through a guard over the project
root, at the project's trust, under the registry's own cap (16 MiB restricted, 64 MiB
trusted; see [Trust — files of the project root](../concepts/trust.md#files-of-the-project-root)).
A missing file is an empty registry. A refused or unreadable one is reported, and
so is one that does not parse (refused for its content, `SecurityReason::Malformed`,
quarantined until the file changes): the index goes on with every asset under its
path's identity, a registry so loaded (`AssetIdRegistry::unread`,
`is_read() == false`) never writes over the file, the editor refuses meanwhile a
rename, a move, a trash and every new file at a path where no file is (a script or
a file made in the asset browser, a duplicate, a scene, material or prefab saved
there: it could not drop a trashed file's entry, so would take it once the registry
reads), and every reserved write, over a file or not: the id a renamed file holds is
unknown, so a prefab saved over itself or a material saved and assigned would name
another. A scene written over is still saved, and a pack build fails rather than ship
a renamed asset under another identity.

**Stamp and save.** The file on disk is the truth. A registry remembers the
**stamp** of what it last read or wrote: the blake3 hash of the file's bytes, or
that there was no file (a content hash, so a write that keeps the length and the
time is still seen). `AssetIdRegistry::follow` compares it with the file: changed,
the file is read again (or, when it does not parse, refused as above and the
registry unread); unchanged, nothing is read. The editor follows the file before
each operation that freezes, drops or saves an identity, and when a reindex lands.
`AssetIdRegistry::save` is a compare-and-swap: it writes only while the file still
has the stamp the registry holds, and the registry then holds the stamp of what it
wrote. A change made outside the editor since it was read — a pull, a merge, its
conflict markers — refuses the save, and is never written over.

**One writer at a time.** A save reads, checks and writes the file under an exclusive
OS lock on `.khora/asset-registry.ron.lock`, a file of its own beside the registry,
since the registry itself is replaced by a rename (`khora_io::fs::PathLock`, the lock
a game build holds on its folder too). The save waits for it: a save is short. Two
editors open on one project therefore never write over what the other saved: the
second reads the file once the first has written it, and its check refuses it until
it follows the file. The lock file is left in place, empty. A program outside the
engine takes no lock (`git`): the check still refuses what it changed before the
save read the file, and only a change it lands between the check and the write is lost.

## `.kmat` — material file

A `.kmat` is a material asset referenced from an entity via
`AssetRef::<Box<dyn Material>>::Asset(uuid)` and resolved through the VFS. The recognised
extensions for the material category are `.kmat` and `.mat`
(`kind_for_extension` maps both to the kind `"material"`).

The on-disk form is **RON** of a type-tagged tree:

```ron
{
    "type_name": "StandardMaterial",
    "material": { /* the concrete material fields */ },
}
```

`type_name` selects a `MaterialRegistration` from an open, inventory-based
registry (`khora-data::scene::material_registration`); the `material`
sub-value is decoded by that registration. Four material types register by
default:

| `type_name` | Type | Workflow |
|---|---|---|
| `StandardMaterial` | PBR metallic-roughness | `base_color`, `metallic`, `roughness`, optional texture maps (base-color, metallic-roughness, normal, emissive, occlusion). |
| `UnlitMaterial` | Unlit | Flat color, no lighting. |
| `EmissiveMaterial` | Emissive | Self-illuminating. |
| `WireframeMaterial` | Wireframe | Edge rendering. |

A material's texture references contribute its dependency list in the pack index:
the `dependencies` of a `.kmat` are the `AssetUUID`s of the textures it references
(see `khora_io::asset::dependencies`). Custom material types become serializable
by registering their own `MaterialRegistration` (the `#[derive(Material)]` macro
generates it). See [Materials and lighting](../how-to/materials-and-lighting.md).

---

*Rationale: [Serialization](../concepts/serialization.md) ·
[Assets](../concepts/assets.md). Tasks:
[Save and load scenes](../how-to/save-and-load-scenes.md) ·
[Load assets](../how-to/load-assets.md).*

# Add an asset decoder

This guide shows you how to **add an asset decoder** — turning the bytes of a new
file format into a typed asset — and register it with every asset service the
engine opens.

**Prerequisites:** you have read [Assets and the VFS](../concepts/assets.md) and
the guard in [Trust](../concepts/trust.md#the-guard): a decoder reads bytes from
files someone else may have made.

## Step 1 — The asset type

The decoder produces a value of a type implementing `khora_core::asset::Asset` (a
marker trait: `impl Asset for HeightMap {}`). Put the type with its domain:
`khora-core` for a format several crates share (`CpuTexture`, `Mesh`), `khora-data`
for one the data layer owns (`SoundData`).

A new format is a new **asset kind**: declare it once, beside the type, with
`register_asset_kind!`. Named like your files' extension, in lowercase ASCII, it
indexes them; with `ergon = "…"`, a script can hold one. The engine refuses at
start a name or an Ergon name declared twice, a name not in lowercase, a name
another kind's extension or a file of no asset already uses (`png`, `bin`), an
Ergon name the language already holds (`int`, `Entity`, `Vec3`, …) and one an
engine component or event holds (`Transform`, `Light`, `Touched`, …).

```rust
khora_core::register_asset_kind!(
    /// A height map, `HeightMap` in Ergon.
    pub const HEIGHTMAP = "heightmap", ergon = "HeightMap"
);
```

## Step 2 — Implement `AssetDecoder`

`AssetDecoder<A>` (`khora-io/src/asset/decoder.rs`) is pure CPU work: a decoder
holds no device and no file. Two methods are required:

- **`sniff(head)`** — whether the first bytes of a file are the start of a file
  this decoder reads: its magic bytes, or valid text for a text format. A file
  whose head it refuses is never decoded, and is quarantined;
- **`load(bytes)`** — the bytes, decoded, or an error.

Three more have defaults, and a decoder of untrusted files should give them:

- **`limits(trust)`** — the limits a file of this format may reach at a trust.
  The default is the strictest of all; a built-in decoder returns its kind's,
  `Limits::of(FileKind::…, trust)`;
- **`load_limited(bytes, source_path, limits)`** — what the reader calls: decode
  within `limits` and return a `LimitExceeded` when one stops it. The default
  ignores the limits and calls `load_at`;
- **`load_at(bytes, source_path)`** — for a format whose file references files
  beside it (a glTF's buffers). The default calls `load`.

```rust
use std::error::Error;
use std::path::Path;

use crate::asset::{AssetDecoder, DecoderRegistration};
use crate::trust::{FileKind, LimitExceeded, Limits, Trust};

/// Decodes `.heightmap` files: "KHM1", width and height as u32 LE, then one f32 per cell.
#[derive(Clone, Default)]
pub struct HeightMapDecoder;

impl AssetDecoder<HeightMap> for HeightMapDecoder {
    fn sniff(&self, head: &[u8]) -> bool {
        head.starts_with(b"KHM1")
    }

    fn limits(&self, trust: Trust) -> Limits {
        Limits::of(FileKind::Other, trust)
    }

    fn load(&self, bytes: &[u8]) -> Result<HeightMap, Box<dyn Error + Send + Sync>> {
        self.load_limited(bytes, Path::new(""), &Limits::strictest())
    }

    fn load_limited(
        &self,
        bytes: &[u8],
        _source_path: &Path,
        limits: &Limits,
    ) -> Result<HeightMap, Box<dyn Error + Send + Sync>> {
        let (width, height) = HeightMap::read_size(bytes)?; // checks the header's length
        let cells = u64::from(width) * u64::from(height);
        if cells > limits.max_elements {
            return Err(Box::new(LimitExceeded {
                what: format!("a {width}×{height} height map"),
                limit: limits.max_elements,
            }));
        }
        Ok(HeightMap::parse(bytes, width, height)?) // your format's parsing
    }
}
```

Validate everything the bytes claim before you allocate for it: a size read from
the file is checked against the limits first. Never `unwrap` on the bytes. A
decoder that panics anyway is caught by the reader — the file is refused, the
engine goes on — but a panic is a bug.

## Step 3 — Register it

Submit a `DecoderRegistration` through `inventory`, in the decoder's file. Its
`kind` is your files' asset kind, and `register` is a plain function (no captures)
that registers the decoder on a service, under the kind's name:

```rust
inventory::submit! {
    DecoderRegistration {
        kind: HEIGHTMAP,
        register: |svc| {
            svc.register_decoder::<HeightMap>(HEIGHTMAP.name(), HeightMapDecoder);
        },
    }
}
```

The kind must be `kind_for_extension(ext)` for your extension, or the reader never
routes your files to the decoder. An extension the engine's table does not list is
the registered kind named like it (`.heightmap` → the kind `heightmap`); with no
such kind, the file is not indexed. To give the format other extensions, add it to
the table in `khora-io/src/asset/extensions.rs`.

Put the file beside the built-in decoders in `crates/khora-io/src/asset/decoders/`
and add it to that module's `mod.rs`, as `font` and `texture` are.

**When it is registered.** Not when the engine starts: when an asset service is
opened over a project's files or a pack (`register_default_decoders`,
`register_packed_decoders`), which register every `inventory` decoder first. A
service made with `AssetService::new` alone has no decoder.

**The two explicit ones.** Sounds and meshes are registered by those two
functions directly (`khora-io/src/asset/defaults.rs`), not through `inventory`. A
mesh decoder needs a resolver for a glTF's buffers and images — on the disk,
guarded, for a project; inside the pack for a shipped game — and the service
gives it the right one. A format whose decoder needs such a dependency follows
that pattern.

## Step 4 — Use the asset

A decoded asset is never stored inline on an entity: it is reached through its
uuid and an `AssetHandle<T>`. The engine resolves on its own only what entities
name through `MeshRef`, `MaterialRef`, a UI font and a UI image. A new asset kind
is resolved by its user: a system that `request`s it from the `AssetService` and
writes the handle where it is needed, or a component holding a
`HandleComponent<T>`. An asset a device must hold — a GPU texture, a buffer —
reaches it through a [residency](../concepts/residency.md).

## Step 5 — Verify it works

Test the decoder on bytes, in its file's tests:

```rust
#[test]
fn a_height_map_decodes_and_a_foreign_file_is_not_sniffed() {
    let bytes = height_map_bytes(2, 2, &[0.0, 1.0, 2.0, 3.0]);
    assert!(HeightMapDecoder.sniff(&bytes));
    assert!(!HeightMapDecoder.sniff(b"\x89PNG"));
    let map = HeightMapDecoder.load(&bytes).expect("a valid height map decodes");
    assert_eq!((map.width, map.height), (2, 2));
}
```

Test a file past the limits too: `load_limited` with a small `max_elements`
returns an error, and allocates nothing for it.

```bash
cargo t -p khora-io heightmap
```

## Related

- [Assets and the VFS](../concepts/assets.md) — the index, the reader and the
  service.
- [Trust](../concepts/trust.md) — the guard, the limits, the quarantine.
- [Residency](../concepts/residency.md) — making a decoded asset usable by a
  device.
- [Serialization](../concepts/serialization.md) — scene files, which are not read
  through decoders.

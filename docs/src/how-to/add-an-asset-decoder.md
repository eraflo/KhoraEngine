# Add an asset decoder

This guide shows you how to **add an asset decoder** — turning raw bytes of a new
format into a typed asset — and have it auto-register with the `AssetService`.

**Prerequisites:** you have read [Assets and VFS](../concepts/assets.md).

## Step 1 — Implement the `AssetDecoder` trait

A decoder is pure CPU work: it holds no GPU/IO state and implements
`AssetDecoder<A>` for the asset type `A` it produces. `load` takes a byte slice and
returns the typed asset or a boxed error. Validate the bytes at this boundary —
decoders sit at a trust boundary (untrusted file input).

```rust
use khora_io::asset::{AssetDecoder, DecoderRegistration};

/// The asset type this decoder produces (must implement `Asset`).
use my_crate::HeightMap;

#[derive(Clone, Default)]
pub struct HeightMapDecoder;

impl AssetDecoder<HeightMap> for HeightMapDecoder {
    fn load(
        &self,
        bytes: &[u8],
    ) -> Result<HeightMap, Box<dyn std::error::Error + Send + Sync + 'static>> {
        let map = HeightMap::parse(bytes)?; // your format parsing
        Ok(map)
    }
}
```

## Step 2 — Auto-register via `DecoderRegistration`

For a slot with a single canonical implementation, submit a `DecoderRegistration`
through `inventory`. The `type_name` is the asset-type string the indexer derives from
the file extension; the `register` function is a plain function pointer (no captures)
that registers the decoder on the `AssetService`:

```rust
inventory::submit! {
    DecoderRegistration {
        type_name: "heightmap",
        register: |svc| {
            svc.register_decoder::<HeightMap>("heightmap", HeightMapDecoder);
        },
    }
}
```

Place the file next to the built-in decoders in
`crates/khora-io/src/asset/decoders/` and re-export it from that module's `mod.rs`,
matching the existing entries (e.g. `texture`, `shader`, `font`). Registration fires
at startup — no manual wiring.

> The `type_name` must match what `IndexBuilder::asset_type_for_extension` returns for
> your file extensions, or the loader will not route bytes to your decoder.
>
> Slots where multiple backends compete (`audio`, `mesh`) deliberately stay registered
> explicitly at the call site instead of through `inventory` — follow the pattern in
> those modules if your format is one of those.

## Step 3 — Reference the asset through a handle

Decoded assets are never stored inline on entities. Load through the asset service and
reference the result with `AssetHandle<T>` / `HandleComponent<T>`; the engine resolves
the handle to the decoded data when needed.

## Step 4 — Verify it works

```bash
cargo test --workspace
```

A unit test can call `decoder.load(&bytes)` directly and assert the parsed asset, or
load a sample file through the `AssetService` and confirm the handle resolves.

## Related

- [Assets and VFS](../concepts/assets.md) — the asset service, VFS, and handle model.
- [Serialization](../concepts/serialization.md) — the three-strategy scene format (distinct
  from asset decoding).

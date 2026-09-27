# Brand fonts

This directory holds the typefaces used by Khora's "Deep Navy / Silver" brand
theme, shared by the editor and the hub:

- **Geist** / **Geist Mono** — proportional + monospace UI text (Vercel).
- **Fraunces** — display serif for screen headings and hero numerals
  (Undercase Type). 72pt optical instances, Regular + SemiBold.
- **Lucide** — icon font reached via `FontFamilyHint::Icons`.

## Expected files

```
Geist-Regular.ttf  Geist-Medium.ttf  Geist-SemiBold.ttf
GeistMono-Regular.ttf  GeistMono-Medium.ttf
Fraunces-Regular.ttf  Fraunces-SemiBold.ttf
Lucide.ttf
LICENSE-Geist.txt  LICENSE-Fraunces.txt  LICENSE-Lucide.txt
```

They are embedded in the binary by `crates/khora-tool-ui/src/fonts.rs`
(`include_bytes!`) and handed to the egui shell as a `FontPack`, so the editor,
the hub, and any other Khora tool get them wherever the binary runs.

## Licenses — all SIL Open Font License 1.1

| Font | Source | Bundled license |
|---|---|---|
| Geist / Geist Mono | <https://github.com/vercel/geist-font> | `LICENSE-Geist.txt` |
| Fraunces | <https://github.com/undercasetype/Fraunces> | `LICENSE-Fraunces.txt` |
| Lucide | <https://github.com/lucide-icons/lucide> | `LICENSE-Lucide.txt` |

If you redistribute a tool, ship the matching `LICENSE-*.txt` with it.

## How to fetch Fraunces (macOS / Linux / Git Bash, from the repo root)

```bash
BASE="https://raw.githubusercontent.com/undercasetype/Fraunces/master"
DST=crates/khora-tool-ui/assets/fonts
curl -sL -o "$DST/Fraunces-Regular.ttf"  "$BASE/fonts/ttf/Fraunces72pt-Regular.ttf"
curl -sL -o "$DST/Fraunces-SemiBold.ttf" "$BASE/fonts/ttf/Fraunces72pt-SemiBold.ttf"
curl -sL -o "$DST/LICENSE-Fraunces.txt"  "$BASE/OFL.txt"
```

Geist and Lucide are already vendored; see this repo's git history for their
original fetch commands.

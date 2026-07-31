# Editor brand fonts

This directory holds the typefaces used by Khora's "Deep Navy / Silver" brand
theme, shared by the editor and the hub:

- **Geist** / **Geist Mono** — proportional + monospace UI text (Vercel).
- **Fraunces** — display serif for screen headings and hero numerals
  (Undercase Type). 72pt optical instances, Regular + SemiBold.
- **Lucide** — icon font reached via `FontFamilyHint::Icons`.

If any file is missing the app still works — egui falls back to its built-in
fonts, Fraunces falls back to Geist, and missing icons render as blank glyphs.

## Expected files

```
Geist-Regular.ttf  Geist-Medium.ttf  Geist-SemiBold.ttf
GeistMono-Regular.ttf  GeistMono-Medium.ttf
Fraunces-Regular.ttf  Fraunces-SemiBold.ttf
Lucide.ttf
LICENSE-Geist.txt  LICENSE-Fraunces.txt  LICENSE-Lucide.txt
```

They are loaded by `crates/khora-editor/src/fonts.rs` through the `FileLoader`
asset I/O layer, then handed to the egui shell as a `FontPack`. The same set
also lives under `hub/assets/fonts/` for the standalone launcher (the hub
reaches the shared brand tokens through `khora-sdk`, but still bundles its own
copy of the font files).

## Licenses — all SIL Open Font License 1.1

| Font | Source | Bundled license |
|---|---|---|
| Geist / Geist Mono | <https://github.com/vercel/geist-font> | `LICENSE-Geist.txt` |
| Fraunces | <https://github.com/undercasetype/Fraunces> | `LICENSE-Fraunces.txt` |
| Lucide | <https://github.com/lucide-icons/lucide> | `LICENSE-Lucide.txt` |

If you redistribute the app with these fonts bundled, keep the matching
`LICENSE-*.txt` alongside them.

## How to fetch Fraunces (macOS / Linux / Git Bash, from the repo root)

```bash
BASE="https://raw.githubusercontent.com/undercasetype/Fraunces/master"
for DST in crates/khora-editor/assets/fonts hub/assets/fonts; do
  curl -sL -o "$DST/Fraunces-Regular.ttf"  "$BASE/fonts/ttf/Fraunces72pt-Regular.ttf"
  curl -sL -o "$DST/Fraunces-SemiBold.ttf" "$BASE/fonts/ttf/Fraunces72pt-SemiBold.ttf"
  curl -sL -o "$DST/LICENSE-Fraunces.txt"  "$BASE/OFL.txt"
done
```

Geist and Lucide are already vendored; see this repo's git history for their
original fetch commands.

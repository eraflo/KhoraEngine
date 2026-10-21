# Brand fonts

The typefaces of every Khora tool, the logo's pair (`docs/src/design/editor.md`,
"Typography"), shared by the editor and the hub:

- **Archivo** — every word on screen. Regular, Medium and SemiBold at normal
  width for text, labels and titles; **Archivo Wide Light** (112 % width) for
  display titles; **Archivo Expanded Light** (125 % width) for the name KHORA.
- **IBM Plex Mono** — code, telemetry, paths, numbers.
- **Noto Sans Mono, Greek** — the Greek block only, a fallback behind Plex
  Mono for `ΧΩΡΑ` under the name.
- **Lucide** — icon font reached via `FontFamilyHint::Icons`.

## Files

```
Archivo-Regular.ttf  Archivo-Medium.ttf  Archivo-SemiBold.ttf
Archivo-Wide-Light.ttf  Archivo-Expanded-Light.ttf
IBMPlexMono-Regular.ttf  IBMPlexMono-Medium.ttf
NotoSansMono-Greek-Light.ttf
Lucide.ttf
LICENSE-Archivo.txt  LICENSE-IBMPlexMono.txt  LICENSE-NotoSansMono.txt  LICENSE-Lucide.txt
```

They are embedded in the binary by `crates/khora-tool-ui/src/fonts.rs`
(`include_bytes!`) and handed to the egui backend as a `FontPack`, so the
editor, the hub, and any other Khora tool get them wherever the binary runs.

## Licenses — all SIL Open Font License 1.1

| Font | Source | Bundled license |
|---|---|---|
| Archivo | <https://github.com/google/fonts/tree/main/ofl/archivo> | `LICENSE-Archivo.txt` |
| IBM Plex Mono | <https://github.com/google/fonts/tree/main/ofl/ibmplexmono> | `LICENSE-IBMPlexMono.txt` |
| Noto Sans Mono | <https://github.com/google/fonts/tree/main/ofl/notosansmono> | `LICENSE-NotoSansMono.txt` |
| Lucide | <https://github.com/lucide-icons/lucide> | `LICENSE-Lucide.txt` |

If you redistribute a tool, ship the matching `LICENSE-*.txt` with it.

## How the files are made

Archivo and Noto Sans Mono are variable fonts upstream, and egui reads no
variation axes, so the files here are static instances cut with
[fontTools](https://github.com/fonttools/fonttools) (`pip install fonttools`).
From the repo root, in Git Bash:

```bash
BASE=https://raw.githubusercontent.com/google/fonts/main/ofl
DST=crates/khora-tool-ui/assets/fonts
TMP=$(mktemp -d)
curl -sSfL -o "$TMP/Archivo.ttf"      "$BASE/archivo/Archivo%5Bwdth%2Cwght%5D.ttf"
curl -sSfL -o "$TMP/NotoSansMono.ttf" "$BASE/notosansmono/NotoSansMono%5Bwdth%2Cwght%5D.ttf"
curl -sSfL -o "$DST/IBMPlexMono-Regular.ttf" "$BASE/ibmplexmono/IBMPlexMono-Regular.ttf"
curl -sSfL -o "$DST/IBMPlexMono-Medium.ttf"  "$BASE/ibmplexmono/IBMPlexMono-Medium.ttf"
curl -sSfL -o "$DST/LICENSE-Archivo.txt"      "$BASE/archivo/OFL.txt"
curl -sSfL -o "$DST/LICENSE-IBMPlexMono.txt"  "$BASE/ibmplexmono/OFL.txt"
curl -sSfL -o "$DST/LICENSE-NotoSansMono.txt" "$BASE/notosansmono/OFL.txt"

I="fonttools varLib.instancer"
$I "$TMP/Archivo.ttf" wdth=100 wght=400 -o "$DST/Archivo-Regular.ttf"
$I "$TMP/Archivo.ttf" wdth=100 wght=500 -o "$DST/Archivo-Medium.ttf"
$I "$TMP/Archivo.ttf" wdth=100 wght=600 -o "$DST/Archivo-SemiBold.ttf"
$I "$TMP/Archivo.ttf" wdth=112 wght=300 -o "$DST/Archivo-Wide-Light.ttf"
$I "$TMP/Archivo.ttf" wdth=125 wght=300 -o "$DST/Archivo-Expanded-Light.ttf"
$I "$TMP/NotoSansMono.ttf" wdth=100 wght=300 -o "$TMP/NotoSansMono-Light.ttf"
pyftsubset "$TMP/NotoSansMono-Light.ttf" --unicodes=U+0370-03FF \
  --name-IDs='*' --notdef-outline --output-file="$DST/NotoSansMono-Greek-Light.ttf"
```

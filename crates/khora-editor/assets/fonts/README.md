# Editor brand fonts

This directory holds the **Geist** typeface family used by the editor's
"Deep Navy / Silver" brand theme.

If the files are missing, the editor still works — the underlying UI library
(egui) falls back to its built-in proportional / monospace fonts.

## Expected files

```
Geist-Regular.ttf
Geist-Medium.ttf
Geist-SemiBold.ttf
GeistMono-Regular.ttf
GeistMono-Medium.ttf
LICENSE-Geist.txt        (the SIL OFL text bundled alongside the .ttf)
```

They are loaded by `crates/khora-editor/src/fonts.rs` through the `FileLoader`
asset I/O layer, then handed to the egui shell as a `FontPack`.

## License

Geist and Geist Mono are released by Vercel under the **SIL Open Font License 1.1**.

- Repository: <https://github.com/vercel/geist-font>
- License text: <https://github.com/vercel/geist-font/blob/main/OFL.txt>

If you redistribute the editor with these fonts bundled, also bundle
`LICENSE-Geist.txt` alongside them.

## How to fetch them (Windows / PowerShell, from the repo root)

```powershell
$tmp = "$env:TEMP\geist.zip"
Invoke-WebRequest -Uri "https://github.com/vercel/geist-font/archive/refs/heads/main.zip" -OutFile $tmp
Expand-Archive -Force -Path $tmp -DestinationPath "$env:TEMP\geist"

$src = "$env:TEMP\geist\geist-font-main\fonts"
$dst = "crates\khora-editor\assets\fonts"
Copy-Item "$src\Geist\ttf\Geist-Regular.ttf"      -Destination $dst -Force
Copy-Item "$src\Geist\ttf\Geist-Medium.ttf"       -Destination $dst -Force
Copy-Item "$src\Geist\ttf\Geist-SemiBold.ttf"     -Destination $dst -Force
Copy-Item "$src\GeistMono\ttf\GeistMono-Regular.ttf" -Destination $dst -Force
Copy-Item "$src\GeistMono\ttf\GeistMono-Medium.ttf"  -Destination $dst -Force
Copy-Item "$env:TEMP\geist\geist-font-main\OFL.txt" -Destination "$dst\LICENSE-Geist.txt" -Force

Remove-Item -Recurse -Force "$env:TEMP\geist", $tmp
```

## How to fetch them (macOS / Linux / Git Bash)

```bash
TMP="${TMPDIR:-/tmp}"
curl -sL -o "$TMP/geist.zip" https://github.com/vercel/geist-font/archive/refs/heads/main.zip
unzip -q -o "$TMP/geist.zip" -d "$TMP/geist"

SRC="$TMP/geist/geist-font-main/fonts"
DST="crates/khora-editor/assets/fonts"
cp "$SRC/Geist/ttf/Geist-Regular.ttf"            "$DST/"
cp "$SRC/Geist/ttf/Geist-Medium.ttf"             "$DST/"
cp "$SRC/Geist/ttf/Geist-SemiBold.ttf"           "$DST/"
cp "$SRC/GeistMono/ttf/GeistMono-Regular.ttf"    "$DST/"
cp "$SRC/GeistMono/ttf/GeistMono-Medium.ttf"     "$DST/"
cp "$TMP/geist/geist-font-main/OFL.txt"          "$DST/LICENSE-Geist.txt"

rm -rf "$TMP/geist" "$TMP/geist.zip"
```

The same five files also live under `hub/assets/fonts/` for the standalone
launcher (the hub has zero engine dependencies, so it bundles its own copy).

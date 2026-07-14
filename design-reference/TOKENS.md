# Khora design tokens — frozen source of truth

This is the canonical token table the egui theme (`khora-core::ui::brand`) is generated from.
The HTML mockups (`khora-editor-mockup.html`, `khora-hub-mockup.html`) are the visual arbiter;
these values reproduce them exactly.

## Color pipeline (why the stored floats are sRGB, not linear)

The single egui mapping site is `khora-infra/src/ui/egui/theme.rs::c()`:

```rust
fn c(color: [f32; 4]) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(
        (color[0] * 255.0) as u8, (color[1] * 255.0) as u8,
        (color[2] * 255.0) as u8, (color[3] * 255.0) as u8,
    )
}
```

`Color32::from_rgba_unmultiplied` takes **sRGB 8-bit** components. So the `[f32; 4]` stored in a
`UiTheme` / `pal` constant must be **sRGB in 0..1** (i.e. `sRGB_byte / 255`), NOT linear-light.
Storing true linear values would render everything too dark. The `LinearRgba` type name is a
historical misnomer here — treat the four floats as sRGB.

### Conversion procedure (frozen)

`oklch(L C H)` → sRGB is done once, offline:

1. OKLCH → OKLab: `a = C·cos(H°·π/180)`, `b = C·sin(H°·π/180)`.
2. OKLab → linear sRGB via the standard Björn Ottosson matrix (`l_,m_,s_` → cube → 3×3 matrix).
3. Linear → sRGB gamma: `c ≥ 0.0031308 ? 1.055·c^(1/2.4) − 0.055 : 12.92·c`.
4. **Per-channel clamp to [0,1]**, then `round(·×255)` for the byte, `round(·×255)/255` for the float.

The reference implementation is `scratchpad/oklch2srgb.mjs`. **Verified**: for every anchor color the
per-channel clamp matches Chrome's `oklch()` canvas rasterization byte-for-byte (silver `191,212,231`,
bg-0 `8,12,21`, gold `231,190,104`, red `246,109,103`, green `114,207,142`, amber `243,174,81`,
cyan `115,204,234`, violet `173,155,246`, ink-0 `241,244,247`). No gamut-mapping divergence at these
chroma levels — the clamp is exact.

## Color tokens

| token | OKLCH | sRGB 8-bit | normalized `[f32;3]` | alpha | role |
|---|---|---|---|---|---|
| `bg-0`        | `0.155 0.020 264` | 8,12,21     | `[0.0314, 0.0471, 0.0824]` | 1    | app void / behind everything |
| `bg-1`        | `0.190 0.023 264` | 15,20,30    | `[0.0588, 0.0784, 0.1176]` | 1    | panel surface |
| `bg-2`        | `0.224 0.026 264` | 21,27,40    | `[0.0824, 0.1059, 0.1569]` | 1    | elevated / rows |
| `bg-3`        | `0.265 0.029 264` | 30,37,52    | `[0.1176, 0.1451, 0.2039]` | 1    | interactive rest |
| `bg-4`        | `0.315 0.033 264` | 41,50,67    | `[0.1608, 0.1961, 0.2627]` | 1    | interactive active / selected fill |
| `line`        | `0.34 0.022 264`  | 50,56,68    | `[0.1961, 0.2196, 0.2667]` | 0.65 | hairline |
| `line-strong` | `0.46 0.028 264`  | 80,88,104   | `[0.3137, 0.3451, 0.4078]` | 0.85 | strong separator / input border |
| `silver`      | `0.86 0.035 245`  | 191,212,231 | `[0.7490, 0.8314, 0.9059]` | 1    | brand / primary action |
| `silver-dim`  | `0.70 0.040 245`  | 138,162,182 | `[0.5412, 0.6353, 0.7137]` | 1    | brand secondary / dim icon |
| `gold`        | `0.82 0.115 84`   | 231,190,104 | `[0.9059, 0.7451, 0.4078]` | 1    | **selection / active only** |
| `green`       | `0.78 0.130 152`  | 114,207,142 | `[0.4471, 0.8118, 0.5569]` | 1    | healthy / success |
| `amber`       | `0.80 0.135 72`   | 243,174,81  | `[0.9529, 0.6824, 0.3176]` | 1    | warning |
| `cyan`        | `0.80 0.095 222`  | 115,204,234 | `[0.4510, 0.8000, 0.9176]` | 1    | info / link |
| `violet`      | `0.74 0.130 292`  | 173,155,246 | `[0.6784, 0.6078, 0.9647]` | 1    | custom agents |
| `red`         | `0.70 0.170 25`   | 246,109,103 | `[0.9647, 0.4275, 0.4039]` | 1    | error / destructive |
| `ink-0`       | `0.965 0.005 250` | 241,244,247 | `[0.9451, 0.9569, 0.9686]` | 1    | text primary |
| `ink-1`       | `0.820 0.010 250` | 191,197,202 | `[0.7490, 0.7725, 0.7922]` | 1    | text secondary |
| `ink-2`       | `0.660 0.013 250` | 140,147,154 | `[0.5490, 0.5765, 0.6039]` | 1    | text muted |
| `ink-3`       | `0.500 0.015 250` | 93,100,108  | `[0.3647, 0.3922, 0.4235]` | 1    | text disabled |
| `text-inverse`| `0.18 0.02 264`   | 13,18,27    | `[0.0510, 0.0706, 0.1059]` | 1    | ink on filled silver (buttons) |

Axis colors reuse the accent tokens: **X = red, Y = green, Z = cyan** (transform fields, view gizmo).

Semantic → `UiTheme` slot mapping for Phase B: `background=bg-0`, `surface=bg-1`, `surface2=bg-2`,
`surface3=bg-3`, `surface_active=bg-4`, `line`/`line_strong`, `primary=silver`, `primary_dim=silver-dim`,
`accent_a=cyan`, `accent_b=violet`, `accent_c=gold` (selection), `status success/warning/error =
green/amber/red`, `axis x/y/z = red/green/cyan`, `text 0..3 = ink-0..3`, **new** `text_inverse`.

## Scalar tokens (unified — one set for hub + editor)

| group | token | value |
|---|---|---|
| radius | `radius_sm` / `radius_md` / `radius_lg` / `radius_xl` | 3 / 5 / 7 / 10 px |
| type   | `font_size_caption` / `body` / `title` / `display`   | 10.5 / 12.5 / 14 / 22 px |
| spacing| `pad_row` / `pad_card`                                | 8 / 16 px (4px grid: 4/8/12/16/20/24/32/40) |

Fonts: **Geist** (400/500/600) proportional, **Geist Mono** (400/500) monospace, **Fraunces**
(400/500 optical) display. Display is used only for screen headings (hub) and Control-Plane metric
numerals; everything else is Geist. Icons: **Lucide** TTF as the `icons` font family (see mapping).

## Icon mapping — mockup (Tabler) → shipping (Lucide)

The HTML uses Tabler via CDN for convenience. The apps ship Lucide (already wired as the `icons`
family via the `Icon` enum → PUA codepoints). Phase B adds any missing `Icon` variants.

| Tabler class (mockup) | Lucide name | Tabler class | Lucide name |
|---|---|---|---|
| `player-play`  | `play`            | `photo`         | `image` |
| `player-pause` | `pause`           | `movie`         | `film` |
| `player-stop`  | `square`          | `box-multiple`  | `boxes` |
| `cube`         | `box`             | `device-floppy` | `save` |
| `cube-off`     | `package-x`       | `file-export`   | `file-output` |
| `cpu`          | `cpu`             | `file-plus`     | `file-plus` |
| `settings`     | `settings`        | `folder`        | `folder` |
| `camera`       | `camera`          | `folder-open`   | `folder-open` |
| `sun`          | `sun`             | `folder-off`    | `folder-x` |
| `bulb`         | `lightbulb`       | `packages`      | `package` |
| `square`       | `square`          | `package`       | `package` |
| `volume`       | `volume-2`        | `download`      | `download` |
| `pointer`      | `mouse-pointer-2` | `refresh`       | `refresh-cw` |
| `arrows-move`  | `move`            | `flask`         | `flask-conical` |
| `rotate`       | `rotate-cw`       | `stack-2`       | `layers` |
| `resize`       | `scaling`         | `brand-github`  | `github` |
| `terminal-2`   | `terminal`        | `arrow-left`    | `arrow-left` |
| `activity`     | `activity`        | `check`         | `check` |
| `info-circle`  | `info`            | `command`       | `command` |
| `circle-check` | `circle-check`    | `pencil`        | `pencil` |
| `alert-triangle`| `triangle-alert` | `copy`          | `copy` |
| `chevron-down` | `chevron-down`    | `focus-2`       | `scan-eye` |
| `chevron-right`| `chevron-right`   | `click`         | `mouse-pointer-click` |
| `palette`      | `palette`         | `x`             | `x` |
| `plus`         | `plus`            | `trash`         | `trash-2` |
| `search`       | `search`          | `file-plus`     | `file-plus` |

## What the mockups cover (0.7 scope — closed list)

**Editor**: token/type/component-state spec pages; Scene shell (titlebar, spine, hierarchy, viewport
with transform gizmo + view-orientation gizmo + stats + camera preview, bottom dock console with
filter pills, inspector with Properties|Debug segmented tabs + collapsible groups + axis fields);
Control Plane shell (metric strip, C/I/O agent groups + health bars, frame-descent rail, agent
inspector + sparkline); Asset browser (folder tree, breadcrumb, tile grid); **states & overlays**
(command palette + no-match, transport Editing/Playing/Paused, context menu, rename-in-place,
drag/drop-target, four empty states, tooltip, title-bar menu, panel resize, GORNA stream tab).

**Hub**: Home (sidebar, project cards) + no-projects empty; New project (radio cards, nested git →
GitHub, step rail) + validation; Engine manager (skeletons, progress, installed rows) + error/fetch-
failure rows + uninstall confirm modal; Settings (GitHub account, engine path) + disconnected +
device-flow; toasts (info/error).

Explicitly **out of 0.7**: tear-off / floating docks, a Settings editor mode (the spine shows the
affordance but it is a no-op for 0.7), Fraunces beyond the two named uses.

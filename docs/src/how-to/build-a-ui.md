# Build a UI

This guide shows you how to lay out UI nodes with text and images using Khora's
Taffy-backed, ECS-driven UI components.

> **Prerequisites.** You can spawn entities and parent them
> ([Spawn entities and move them](./spawn-and-transform.md)). UI runs through the
> `LayoutSystem` backend your app wired in the bootstrap (Taffy, as the sandbox does).
>
> **Note.** The `UiAgent` runs in **editor mode** today; in-game (play-mode) UI is on the
> roadmap. The components below are the stable vocabulary either way.

## Where the UI types live

UI is just ECS — entities with UI components, in the same `World` as everything else. The
components are in `khora_sdk::khora_data::ui`:

```rust
use khora_sdk::khora_data::ui::{
    UiNode, UiStyle, UiText, UiImage, UiBorder, UiVal, UiFlexDirection,
};
```

Layout uses `UiNode` (a flexbox node, like a CSS box); the layout system computes a
screen-space `UiTransform` for each node. Hierarchy uses the same `Parent` / `Children`
components as the 3D scene.

## Lay out a panel

`UiNode` is built as a struct literal — set the sizing, padding, and flex fields you need
and default the rest. `UiStyle` gives it a background and border. Spawn the panel as one
entity:

```rust
use khora_sdk::khora_data::ui::{UiNode, UiStyle, UiVal, UiFlexDirection};
use khora_sdk::prelude::math::Vec4;

let panel = world.spawn((
    UiNode {
        width: UiVal::Px(400.0),
        height: UiVal::Px(300.0),
        flex_direction: UiFlexDirection::Column,
        ..Default::default()
    },
    UiStyle {
        background_color: Vec4::new(0.1, 0.1, 0.12, 0.9),
        border_color: Vec4::new(1.0, 1.0, 1.0, 1.0),
        border_width: 1.0,
        ..Default::default()
    },
));
```

`UiVal` expresses sizes as `Px`, percentages, or auto — see the
[UI reference](../reference/sdk.md) for the full set. `flex_direction`, `flex_grow`, and
`flex_shrink` on `UiNode` drive child arrangement, exactly like flexbox.

## Add text

`UiText` holds the string, a font UUID, a pixel size, and an RGBA color. Spawn it as a
child node and parent it under the panel:

```rust
use khora_sdk::khora_data::ui::{UiNode, UiText};
use khora_sdk::prelude::math::Vec4;
use khora_sdk::prelude::AssetUUID;

let label = world.spawn((
    UiNode::default(),
    UiText {
        content: "Hello, world.".to_owned(),
        font: AssetUUID::new_v5("fonts/inter.ttf"),
        size: 14.0,
        color: Vec4::new(1.0, 1.0, 1.0, 1.0),
    },
));
world.set_parent(label, Some(panel));
```

## Add an image

`UiImage` references a texture by UUID; the layout node sizes it:

```rust
use khora_sdk::khora_data::ui::{UiImage, UiNode, UiVal};
use khora_sdk::prelude::AssetUUID;

let icon = world.spawn((
    UiNode {
        width: UiVal::Px(48.0),
        height: UiVal::Px(48.0),
        ..Default::default()
    },
    UiImage { texture: AssetUUID::new_v5("ui/coin.png") },
));
world.set_parent(icon, Some(panel));
```

## Update UI each frame

For dynamic UI (a HUD counter, a health bar), mutate the components in `update` — the
layout re-runs when components change:

```rust
use khora_sdk::khora_data::ui::UiText;

for (text,) in world.query_mut::<(&mut UiText,)>() {
    text.content = format!("Score: {score}");
}
```

## What the UI agent and lane do

The `UiAgent` owns one lane — `UiRenderLane`. Layout is not a lane: the laid-out `UiScene` is produced by `UiFlow` during the Substrate Pass. (A layout lane reads the UI components, runs Taffy, and
produces a laid-out `UiScene`; a render lane rasterizes that scene over the 3D frame.
Swapping the layout backend is a matter of implementing `LayoutSystem` — see
[UI](../concepts/ui.md).

## Expected result

The panel lays out at its size with its background and border; the text and image appear
inside it, arranged by the flex direction. Mutating `UiText.content` updates the rendered
string on the next frame.

## Related

- [Load and reference assets](./load-assets.md) — fonts and UI textures by UUID.
- [UI](../concepts/ui.md) — the layout/render split and the `LayoutSystem` seam.
- [UI component reference](../reference/sdk.md) — every `Ui*` field and `UiVal` variant.

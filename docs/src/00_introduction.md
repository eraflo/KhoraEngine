# Khora Engine

**An engine that thinks.**

A documentation set for the Khora Engine — an experimental Rust game engine built on a self-optimizing **Symbiotic Adaptive Architecture**. This book captures the philosophy, the architecture, the subsystems, and the SDK, with the same instrumental voice as the editor it ships with.

- Document — Khora Engine Documentation v1.0
- Status — Living document
- Date — May 2026

---

## Contents

1. What Khora is
2. The problem with rigid engines
3. The Khora answer
4. Who this book is for
5. How to read this book
6. Status

---

## 01 — What Khora is

Khora is an experimental real-time engine, written in Rust on edition 2024, organized as a Cargo workspace of eleven crates. It renders through **wgpu 28.0** (Vulkan / Metal / DX12), simulates physics through **Rapier3D**, mixes audio through **CPAL**, lays out UI through **Taffy**, and stores entities through **CRPECS** — a custom archetype-based ECS.

What sets it apart is not the parts list. It is what those parts *do together*. Every major subsystem in Khora is an **agent** with a sense of cost, a sense of options, and a willingness to negotiate. A central observer — the **Dynamic Context Core** — watches the engine's behavior in real time, tracks thermal headroom, frame-time stutter, battery, GPU pressure, and trades resource budgets with each agent through a protocol called **GORNA**. The agents adapt; the work continues.

Most engines decide at compile time. Khora decides at runtime, every tick.

## 02 — The problem with rigid engines

Modern game engines are **rigid**. They impose static pipelines, force developers into manual per-platform tuning, and adapt poorly to hardware diversity — from high-end PCs to mobile and VR.

| Problem | Impact |
|---|---|
| Static resource allocation | Underutilization or bottlenecks |
| Manual per-platform tuning | Tedious, fragile, expensive |
| No contextual awareness | Cannot prioritize what matters to the player right now |

The result is a class of engines that perform well in their default configuration on one target platform, and progressively worse everywhere else.

## 03 — The Khora answer

Khora replaces the rigid orchestrator with a council of intelligent, collaborating agents.

- **Automated self-optimization.** The engine detects bottlenecks and reallocates resources autonomously.
- **Strategic flexibility.** Rendering switches techniques based on system load, with no developer intervention. Physics shrinks its tick rate when the GPU is starving. Audio sheds voices when memory tightens.
- **Goal-oriented decisions.** Every adaptation is driven by a high-level goal — *maintain 90 fps in VR*, *conserve battery on mobile*, *prioritize physics in this volume*.

The architecture has a name: **Symbiotic Adaptive Architecture (SAA)**. Its concrete implementation has another name: the **CLAD** layering — Control, Lanes, Agents, Data. The first describes the *why*. The second describes the *how*. They are two views of the same thing.

The full philosophy lives in [Principles](./01_principles.md). The crate-by-crate map of where SAA becomes CLAD lives in [Architecture](./02_architecture.md).

## 04 — Who this book is for

Two audiences, equally served:

| Audience | What you get |
|---|---|
| **Game developers** | A clean SDK, a `GameWorld` facade over the ECS, a `cargo run -p sandbox` you can copy from. The engine handles the performance problem; you focus on the creative one. Start at [SDK quickstart](./16_sdk_quickstart.md). |
| **Engine contributors** | A complete map of CLAD, the trait surface that holds it together, the rationale behind every layer. Start at [Architecture](./02_architecture.md), then read the per-subsystem chapters. |

Most chapters in the **Subsystems** section are split into a *For game developers* part and a *For engine contributors* part. The split is explicit. You can skip the half that is not yours.

## 05 — How to read this book

If you have never seen Khora before, read in order — at least up to chapter 04. The first five chapters establish vocabulary you will need everywhere else.

If you are evaluating Khora for a game, jump to:
- [Principles](./01_principles.md) for the *why*.
- [SDK quickstart](./16_sdk_quickstart.md) for a working program.
- [Roadmap](./roadmap.md) for what is committed and what is planned.

If you are extending Khora — writing a custom agent, lane, or backend — read:
- [Agents](./06_agents.md) and [Lanes](./07_lanes.md) for the contracts.
- [Extending Khora](./19_extending.md) for a worked example.
- [Decisions](./decisions.md) for the constraints we accept and reject.

If you are interested in the editor, read:
- [Editor](./18_editor.md) for the panels and play mode.
- [Editor design system](./design/editor.md) for the visual language and the voice.

## 06 — Status

Khora is **experimental**. The architecture is stable enough to support a sandbox application, an editor, a play-mode loop, and ~470 workspace tests. The SDK surface is intentionally narrow and will grow as the engine matures.

The Roadmap lays out the multi-year path: scene and assets, then the adaptive core (DCC and GORNA in earnest), then tooling and scripting, then advanced intelligence — and, in a later phase, a native physics solver replacing the third-party backend.

This book ships with the engine. When the engine changes, the book changes in the same commit. When something is uncertain, the [Open questions](./open_questions.md) chapter is honest about it.

---

*An engine that thinks. A book that says so plainly.*

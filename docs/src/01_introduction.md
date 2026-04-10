<div class="hero-banner">

# Khora Engine

An experimental game engine built on a Symbiotic Adaptive Architecture

</div>

## The Problem with Traditional Engines

Modern game engines are fundamentally **rigid**. They impose static pipelines, force developers into complex manual optimization, and adapt poorly to hardware diversity — from high-end PCs to mobile and VR.

| Problem | Impact |
|---------|--------|
| Static resource allocation | Underutilization or bottlenecks |
| Manual per-platform tuning | Tedious, fragile, expensive |
| No contextual awareness | Can't prioritize what matters to the player |

## The Khora Solution

Khora replaces the rigid, top-down orchestrator with a **council of intelligent, collaborating agents**.

<div class="callout callout-info">

**Symbiotic Adaptive Architecture (SAA)** — The engine observes, learns, and continuously adapts to its environment and workload.

</div>

### Core Principles

- **Automated self-optimization** — Detects bottlenecks and reallocates resources autonomously
- **Strategic flexibility** — Rendering can switch techniques based on system load, no developer intervention needed
- **Goal-oriented decisions** — All adaptations driven by high-level goals: "maintain 90fps in VR", "conserve battery on mobile"

## Who is Khora For?

| Audience | Benefit |
|----------|---------|
| **Game developers** | Focus on creative vision, trust the engine to handle performance |
| **Engine contributors** | Clean CLAD architecture, well-defined interfaces, extensible by design |
| **Researchers** | Experimental platform for adaptive systems, GORNA protocol, agent-based orchestration |

<div class="callout callout-tip">

**Getting started** — Read the [Architecture section](./02_saa_philosophy.md) to understand SAA, then jump to the [SDK Guide](./11_sdk_guide.md) to start building.

</div>

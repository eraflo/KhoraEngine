---
trigger: always_on
---

# AI Guidelines for KhoraEngine

> [!IMPORTANT]
> **ALWAYS** consult these guidelines and the project documentation before making changes (voir @docs folder)
> **ALWAYS** follow the principles defined in the documentation.
> **ALWAYS** use the snippets provided in the `.vscode` directory to implement the changes.
> **ALWAYS** consider that you are building things fro a real engine, and provide the most advanced and performant solutions.
> **ALWAYS** finish a feature by adding tests.
> **ALWAYS** use the `cargo xtask` commands to build, test, and check the code.

## Documentation Resources

- **Primary Source**: `docs/` directory (contains all project documentation).
- **Narrative Documentation**: `docs/src/` (source for mdBook).
- **Online Book**: [https://eraflo.github.io/KhoraEngine/](https://eraflo.github.io/KhoraEngine/)
- **API Reference**: [https://eraflo.github.io/KhoraEngine/api/index.html](https://eraflo.github.io/KhoraEngine/api/index.html) (or generate locally via `cargo doc`).
- **Contributing Guidelines**: `CONTRIBUTING.md`

## Core Principles (SAA & CLAD)

KhoraEngine is built on the **Symbiotic Adaptive Architecture (SAA)** implemented via the **CLAD Pattern**.

### The 7 Pillars of SAA
1.  **Dynamic Context Core (DCC)**: The central nervous system maintaining a situational model.
2.  **Intelligent Subsystem Agents (ISAs)**: Semi-autonomous components (Rendering, Physics, etc.) that negotiate for resources.
3.  **GORNA (Goal-Oriented Resource Negotiation & Allocation)**: The protocol for dynamic resource budgeting.
4.  **Adaptive Game Data Flows (AGDF)**: Dynamic data layout via **CRPECS**.
5.  **Semantic Interfaces & Contracts**: Formal Rust traits defining capabilities and requirements (`khora_core`).
6.  **Observability & Traceability**: "Glass Box" philosophy; every decision must be logged with context.
7.  **Developer Guidance**: The engine serves the developer (Learning, Stable, and Manual modes).

### CLAD Implementation Mapping

| SAA Concept | Crate | Responsibility |
| :--- | :--- | :--- |
| **DCC & GORNA** | `khora_control` | Strategic brain, budgeting. |
| **ISAs** | `khora_agents` | Tactical managers. |
| **Strategies** | `khora_lanes` | Fast, deterministic execution paths. |
| **AGDF** | `khora_data` | Data layout, CRPECS. |
| **Contracts** | `khora_core` | Universal traits/types. |
| **Telemetry** | `khora_telemetry` | Observability. |
| **Infra** | `khora_infra` | Hardware/OS bridge. |

## Workflow Instructions

1.  **Check Docs First**: Before implementing a feature or fix, verify if it aligns with the SAA/CLAD principles.
2.  **Respect the Architecture**: Do not bypass the SAA (e.g., don't hardcode resource usage without negotiation unless in a specific "Lane" context).
3.  **Update Docs**: If you change code, update the corresponding `mdBook` entry or rustdoc.

## Quality Checks

Before every commit, run:
```bash
cargo xtask all  # build, test, check, format, clippy
```

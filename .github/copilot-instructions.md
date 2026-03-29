# Khora Engine — GitHub Copilot Instructions

## Reference Documents

All detailed instructions, rules, and architecture documentation live in the repository. Read these files for full context before making any changes:

| File | Content |
|------|---------|
| [`RULES.md`](../RULES.md) | Must-always and must-never coding rules |
| [`AGENTS.md`](../AGENTS.md) | Agent system, constraints, architecture overview, specialized agent personas |
| [`CLAUDE.md`](../CLAUDE.md) | Key types, build commands, frame lifecycle, subsystem details |
| [`SOUL.md`](../SOUL.md) | Agent identity, values, communication style, domain expertise |
| [`INSTRUCTIONS.md`](../INSTRUCTIONS.md) | Quick-reference commands and critical file locations |

## Memory

Persistent working memory for this workspace is stored in [`memory/`](../memory/):

- [`memory/MEMORY.md`](../memory/MEMORY.md) — current state, known issues, architecture decisions
- [`memory/runtime/context.md`](../memory/runtime/context.md) — project context and build commands
- [`memory/runtime/key-decisions.md`](../memory/runtime/key-decisions.md) — architectural decisions log
- [`memory/runtime/dailylog.md`](../memory/runtime/dailylog.md) — session activity log

**Always read `memory/MEMORY.md` at the start of a session and update it when state changes (new issues found, decisions made, work completed). Never use system-level memory paths — this workspace's `memory/` folder is the sole persistent store.**

## Respond in the user's language (French or English).

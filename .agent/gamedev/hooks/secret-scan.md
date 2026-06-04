# Hook — secret-scan (pre-commit)

**Purpose:** block any commit that would introduce a secret into your game's repo. Installed by the AI
installer into a managed block of `.git/hooks/pre-commit`; runs `khora-ai.mjs scan-secrets --staged`.

## What it scans
Staged files for high-confidence patterns: private keys, AWS `AKIA…`/secret keys, GCP service-account JSON,
GitHub/Slack/Google tokens, `.env` and `*.pem`/`*.key`/`id_*` files, and generic
`password|secret|api_key|token = "<value>"` (placeholders ignored).

## Behavior
- **Match → commit blocked**, printing `file:line` + rule.
- False positive → append `# khora-ai:allow-secret` to the line.
- A shipped build is readable by players: **never** bake a real secret in. If one was committed, **rotate it**.

See [`../security-privacy.md`](../security-privacy.md) for the full policy.

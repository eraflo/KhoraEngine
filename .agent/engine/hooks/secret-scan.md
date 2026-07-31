# Hook — secret-scan (pre-commit)

**Purpose:** block any commit that would introduce a secret or confidential value. Installed by the AI
installer into a managed block of `.git/hooks/pre-commit`; runs `khora-ai.mjs scan-secrets --staged`.

## What it scans
Staged files (added/modified), skipping binaries and the `.git/` dir, for high-confidence patterns:

- Private keys — `-----BEGIN ... PRIVATE KEY-----`
- AWS — `AKIA[0-9A-Z]{16}`, `aws_secret_access_key`
- GCP service-account JSON — a `service_account` type field next to a real PEM private-key block
- GitHub / generic tokens — `ghp_…`, `github_pat_…`, `xox[baprs]-…` (Slack), bearer/`Authorization:` secrets
- `.env` files and `*.pem` / `*.key` / `id_rsa` being committed
- Generic `password|secret|api[_-]?key|token = "<value>"` with a non-placeholder value

## Behavior
- **Match → exit non-zero**, printing `file:line` and the rule, and the commit is blocked.
- A confirmed false positive can be allowlisted inline with a trailing `# khora-ai:allow-secret` comment.
- This is a backstop, not a substitute for care: never stage a real secret. If one was ever committed,
  **rotate it** — removing it later does not un-leak it.

See [`../security-privacy.md`](../security-privacy.md) for the full policy.

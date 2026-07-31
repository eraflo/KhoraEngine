# Khora SDK — Security & Privacy (gamedev profile)

Guardrails for building and shipping a game. Enforced at commit time by the
[`secret-scan`](./hooks/secret-scan.md) hook.

---

## A — No dangerous code
- No unsolicited destructive operations (deleting the player's files, wiping save dirs) — never without
  explicit, in-context consent.
- Validate external input: save files, downloaded content, asset bytes, network packets are untrusted —
  check sizes/ranges/formats before use. Never `unwrap()` on them.
- No hidden network exfiltration. If your game talks to a server, it's an explicit, documented feature.
- Refuse to implement malware, cheats-as-a-service, or anything whose purpose is harm.

## B — Never embed or commit secrets
**Never** put a secret in game source, configs, assets, or a shipped build:
- API keys, tokens, passwords, private keys, signing certificates.
- Backend/server credentials, third-party service keys, analytics tokens.
- Personal data or local machine paths (`C:\Users\<name>\…`), internal URLs.

Rules of thumb:
- A shipped game binary/pack is **readable by players** — anything compiled in is effectively public.
  Keep secrets server-side; the client gets short-lived, scoped tokens at runtime, never baked-in keys.
- Load secrets from the environment or an ignored local file — never hard-code them.
- For tests/fixtures use obvious fakes (`"test-key"`), never live values.
- If a secret was ever committed, **rotate it** — deleting it from a later commit does not un-leak it.
- The `secret-scan` pre-commit hook is the backstop, not a substitute for care.

## C — When you spot a problem
Stop and surface it with the file path and a one-line risk. Flag real issues rather than silently
working around them.

# Khora Engine — Security & Privacy (engine profile)

Cross-cutting guardrails. Referenced by [`RULES.md`](./RULES.md) (§8 Must never) and the
[`security-auditor`](./agents/security-auditor.md) agent. Enforced at commit time by the
[`secret-scan`](./hooks/secret-scan.md) hook.

- Document — Khora Security & Privacy v1.0
- Status — Authoritative

---

## A — No dangerous code

- **No unsolicited destructive operations.** Never delete files/branches, wipe directories, or run
  `git reset --hard` / force-push unless the user explicitly asked, in this conversation, for that
  exact action. When in doubt, ask.
- **`unsafe` is justified and bounded.** Every `unsafe` block carries a `// SAFETY:` comment proving
  the invariant. No `unsafe` for convenience; prefer safe abstractions. Audit via the security-auditor.
- **No network exfiltration.** Engine code does not phone home, upload telemetry to third parties, or
  open outbound connections that aren't part of an explicit, reviewed feature.
- **Validate at boundaries.** User input, file I/O, asset bytes, and GPU results are untrusted — check
  lengths, ranges, and formats before use (see `IndexBuilder` / decoder error handling for the pattern).
- **Refuse malicious use.** Do not implement malware, DoS tooling, detection evasion, or anything whose
  primary purpose is harm. Security *hardening* and tests are welcome; offensive tooling is not.
- **Supply chain.** New dependencies are a decision, not a default — prefer the workspace's existing
  crates. Flag any new crate to the user; never add one to satisfy a trivial need.

## B — Privacy & confidentiality (never push secrets)

**Never commit, stage, or push** any of the following — not in code, comments, tests, fixtures,
configs, logs, or commit messages:

- API keys, tokens, passwords, private keys (`BEGIN … PRIVATE KEY`), OAuth secrets.
- Cloud credentials (AWS `AKIA…` / secret keys, GCP service-account JSON, Azure connection strings).
- `.env` files, `*.pem`, `*.key`, `id_rsa`, credential stores, signing certificates.
- Personal data or local machine identity (absolute home paths like `C:\Users\<name>\…`, emails, IPs).
- Internal/private URLs, dashboards, or infrastructure hostnames.

Rules of thumb:

- Secrets come from the environment or an ignored local file at runtime — never hard-coded.
- If a secret is *needed* for a test, use an obvious fake (`"test-key-not-real"`), never a live value.
- Before any commit/push, run the security-auditor or rely on the `secret-scan` pre-commit hook; if a
  real secret was ever committed, **rotate it** — removing it from a later commit is not enough.
- A shipped game build (see the gamedev profile) must never embed engine or developer secrets.

## C — When you spot a problem

- Stop and surface it to the user with the file path and a one-line risk description.
- For an out-of-scope but real vulnerability you noticed while reading unrelated code, flag it rather
  than silently fixing or ignoring it.
- Treat anything that contradicts how a file was described to you (e.g. a "config" that holds live
  credentials) as a red flag worth raising before proceeding.

---

*Security is a must-never, not a nice-to-have. When unsure, ask.*

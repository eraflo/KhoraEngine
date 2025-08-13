# Pre-Push Verification Scripts

This document explains how to use the local verification scripts to ensure the KhoraEngine codebase is clean, consistent, and secure before pushing changes.

## Overview
Two platform-specific helper scripts live at the repository root:

- `verify.ps1` (Windows / PowerShell)
- `verify.sh` (Linux / macOS / Bash)

They perform (in order):
1. Toolchain check (rustc, cargo)
2. Formatting validation (`cargo fmt -- --check`)
3. Linting (`cargo clippy` with warnings as errors)
4. Debug build (`cargo build`)
5. Test suite (`cargo test`)
6. License header verification (ensures each `.rs` starts with the standard copyright line)
7. Dependency vulnerability audit (`cargo audit`, if installed)
8. License / advisory / bans check (`cargo deny`, if installed)
9. Release build size report (compiles release and prints key artifact sizes)

The scripts exit immediately on the first failing mandatory step.

## Windows Usage (PowerShell)
Run a basic check:
```
powershell -ExecutionPolicy Bypass -File .\verify.ps1
```
Optional flags:
- `--install-tools` : Automatically install missing `cargo-audit` / `cargo-deny` if absent.
- `--full` : Treat security / license tool findings as hard failures (exit non-zero). Without this flag, findings are shown as warnings.

Example (strict + auto-install):
```
powershell -ExecutionPolicy Bypass -File .\verify.ps1 --install-tools --full
```

## Linux / macOS Usage (Bash)
Make executable once:
```
chmod +x ./verify.sh
```
Run:
```
./verify.sh
```
Optional flags:
- `--fix` : Automatically apply `cargo fmt --all` before validation.
- `--no-license` : Skip license header check (rarely needed; e.g., bulk regeneration).
- `--install-tools` : Install missing `cargo-audit` / `cargo-deny`.
- `--full` : Fail the run if `cargo audit` or `cargo deny` report issues.

Strict with fixes and tool installation:
```
./verify.sh --fix --install-tools --full
```

## Security & Licensing Tools
| Tool | Purpose | Mandatory? | Becomes Mandatory With `--full` |
|------|---------|------------|----------------------------------|
| cargo-audit | Vulnerability advisories in dependency tree | No | Yes |
| cargo-deny  | License, multiple version, bans, advisories  | No | Yes |

If you want to enforce these in CI by default, run scripts with `--full` in the pipeline.

## License Header Standard
Expected first line for each Rust source file:
```rust
// Copyright 2025 eraflo
```
If missing, the script lists offending files. Add the Apache 2.0 header (see existing files) and rerun.

## Binary Size Report
The scripts perform a release build and print sizes (KB) for:
- `sandbox` / `sandbox.exe`
- `libkhora_engine_core*`

Use this to watch for unexpected size regressions before pushing.

## Recommended Git Hook (Optional)
Create `.git/hooks/pre-push` (Linux/macOS) or `.git/hooks/pre-push.ps1` (Windows) invoking the script. Example Bash hook:
```bash
#!/usr/bin/env bash
set -e
./verify.sh --full || exit 1
```
Windows PowerShell hook example (`.git/hooks/pre-push` + Git config core.hooksPath pointing to a directory with this file):
```powershell
powershell -ExecutionPolicy Bypass -File "$PSScriptRoot/../../verify.ps1" --full
if ($LASTEXITCODE -ne 0) { exit 1 }
```
Make sure hook files are executable on Unix: `chmod +x .git/hooks/pre-push`.

## When to Use
- Before every push (recommended)
- Before opening or updating a pull request
- Prior to tagging a release

## Troubleshooting
| Symptom | Resolution |
|---------|------------|
| Formatting step fails | Run `cargo fmt --all` (or `./verify.sh --fix`) |
| Missing `cargo-audit` / `cargo-deny` | Add `--install-tools` flag |
| Security warnings shown but script succeeds | Re-run with `--full` to enforce failure |
| License header failures | Copy the header from an existing file; ensure year & owner correct |
| Slow first run | Caches build artifacts; subsequent runs much faster |

## Extending
Potential future additions:
- `cargo udeps` (unused dependency detection)
- WASM target build sanity
- Documentation build (`cargo doc --no-deps`)
- Benchmark smoke tests

Contributions welcome—update this doc when extending the scripts.

---
Maintains a consistent, secure, and traceable code base prior to remote integration.

#!/usr/bin/env bash
# KhoraEngine unified pre-push verification (Linux / macOS)
# Usage:
#   chmod +x ./verify.sh
#   ./verify.sh            # run all checks
#   ./verify.sh --fix      # auto-run cargo fmt before checking
#   ./verify.sh --no-license  # skip license header verification
#   ./verify.sh --clean    # clean cache before clippy for stricter checking
#   ./verify.sh --full     # enable security tools (audit/deny)
#   ./verify.sh --install-tools  # auto-install missing tools
# Exits non-zero on first failure.

set -euo pipefail
IFS=$'\n\t'

START_TS=$(date +%s)
DO_FIX=false
CHECK_LICENSE=true
FULL=false
INSTALL=false
CLEAN=false
YEAR="2025"
LICENSE_PATTERN="// Copyright ${YEAR} eraflo"

for arg in "$@"; do
  case "$arg" in
  --fix) DO_FIX=true ; shift ;;
  --no-license) CHECK_LICENSE=false ; shift ;;
  --full) FULL=true ; shift ;;
  --install-tools) INSTALL=true ; shift ;;
  --clean) CLEAN=true ; shift ;;
    *) echo "Unknown arg: $arg"; exit 2 ;;
  esac
done

info()  { printf "\e[33m[INFO]\e[0m %s\n" "$*"; }
success(){ printf "\e[32m[SUCCESS]\e[0m %s\n" "$*"; }
fail()  { printf "\e[31m[FAIL]\e[0m %s\n" "$*"; exit 1; }

info "KhoraEngine pre-push verification (Rust)"

# 1. Toolchain
info "Toolchain versions"
command -v rustc >/dev/null || fail "rustc not found"
command -v cargo >/dev/null || fail "cargo not found"
rustc --version || fail "rustc issue"
cargo --version || fail "cargo issue"

# 2. Format
if $DO_FIX; then
  info "Running cargo fmt (fix)"
  cargo fmt --all || fail "cargo fmt failed"
fi
info "Checking formatting"
if ! cargo fmt --all -- --check; then
  fail "Formatting issues (run: cargo fmt --all or ./verify.sh --fix)"
fi
success "Formatting ok"

# 3. Clippy
info "Running clippy (warnings = errors)"
if $CLEAN; then
  info "Cleaning cache first..."
  cargo clean || fail "cargo clean failed"
fi
if ! cargo clippy --workspace --all-targets --all-features -- -D warnings; then
  fail "Clippy failed"
fi
success "Clippy ok"

# 4. Build
info "Building workspace"
if ! cargo build --workspace; then
  fail "Build failed"
fi
success "Build ok"

# 5. Tests
info "Running tests"
if ! cargo test --workspace --all-features --quiet; then
  fail "Tests failed"
fi
success "Tests ok"

# 6. License headers
if $CHECK_LICENSE; then
  info "Checking license headers"
  MISSING=()
  while IFS= read -r -d '' file; do
    # Skip generated inside target
    [[ "$file" == *"/target/"* ]] && continue
    first_line=$(head -n1 "$file") || first_line=""
    if [[ "$first_line" != "$LICENSE_PATTERN" ]]; then
      MISSING+=("$file")
    fi
  done < <(find . -type f -name '*.rs' -print0)
  if ((${#MISSING[@]})); then
    printf "\e[31mMissing license header in:\n"
    printf '  %s\n' "${MISSING[@]}"
    fail "License header check failed"
  else
    success "License headers ok"
  fi
else
  info "License header check skipped (--no-license)"
fi

# 7. Git status (dirty check)
info "Checking working tree cleanliness"
if ! git diff --quiet || ! git diff --cached --quiet; then
  fail "Uncommitted changes present (commit or stash before push)"
fi
success "Working tree clean"

# 8. Dependency vulnerability audit (cargo-audit)
info "cargo audit (vulnerabilities)"
if ! command -v cargo-audit >/dev/null 2>&1; then
  if $INSTALL; then
    info "Installing cargo-audit"
    cargo install cargo-audit >/dev/null 2>&1 || fail "Failed to install cargo-audit"
  fi
fi
if command -v cargo-audit >/dev/null 2>&1; then
  if ! cargo audit -q; then
    if $FULL; then fail "cargo audit reported issues"; else info "Warn: audit issues (pass --full to fail)"; fi
  else
    success "Audit ok"
  fi
else
  if $FULL; then fail "cargo-audit missing (use --install-tools)"; else info "Skipped (cargo-audit not installed)"; fi
fi

# 9. cargo-deny (licenses / advisories / bans)
info "cargo deny check"
if ! command -v cargo-deny >/dev/null 2>&1; then
  if $INSTALL; then
    info "Installing cargo-deny"
    cargo install cargo-deny >/dev/null 2>&1 || fail "Failed to install cargo-deny"
  fi
fi
if command -v cargo-deny >/dev/null 2>&1; then
  if ! cargo deny check -q; then
    if $FULL; then fail "cargo-deny reported issues"; else info "Warn: deny issues (pass --full to fail)"; fi
  else
    success "Deny ok"
  fi
else
  if $FULL; then fail "cargo-deny missing (use --install-tools)"; else info "Skipped (cargo-deny not installed)"; fi
fi

# 10. Binary size (release)
info "Building release (size report)"
cargo build --workspace --release -q || fail "Release build failed"
ARTIFACTS=$(find target/release -maxdepth 1 -type f \( -name 'sandbox' -o -name 'sandbox.exe' -o -name 'libkhora_engine_core*' \) 2>/dev/null || true)
if [[ -n "$ARTIFACTS" ]]; then
  while IFS= read -r f; do
    sz=$(stat -c%s "$f" 2>/dev/null || stat -f%z "$f")
    kb=$(( (sz + 1023)/1024 ))
    printf "  %s - %s KB\n" "$(basename "$f")" "$kb"
  done <<<"$ARTIFACTS"
else
  info "No release artifacts for size report"
fi

# Summary
elapsed=$(( $(date +%s) - START_TS ))
success "All checks passed in ${elapsed}s"
info "Next: git push (after commit)"
info "Hints: use --install-tools to auto-install audit/deny; --full to fail on security warnings." 

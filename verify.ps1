# Requires PowerShell 5+ (Windows default)
# Run:  powershell -ExecutionPolicy Bypass -File .\verify.ps1
# Purpose: Local pre-push gate (format, lint, test, license headers, summary)

$ErrorActionPreference = 'Stop'
$stopwatch = [System.Diagnostics.Stopwatch]::StartNew()

Write-Host '=== KhoraEngine Pre-Push Verification ===' -ForegroundColor Cyan

# Flags (simple parsing)
$Full = $false      # require security tools to pass
$Install = $false   # auto-install missing cargo tools (audit / deny)
foreach ($a in $args) {
    switch ($a) {
        '--full' { $Full = $true }
        '--install-tools' { $Install = $true }
        default { Write-Host "Unknown arg: $a" -ForegroundColor Red; exit 2 }
    }
}

function Fail($msg) {
    Write-Host "[FAIL] $msg" -ForegroundColor Red
    exit 1
}

# 1. Rust toolchain sanity
Write-Host '[1/9] Toolchain check' -ForegroundColor Yellow
try {
    $rustcV = (rustc --version)
    $cargoV = (cargo --version)
    Write-Host " rustc: $rustcV" -ForegroundColor DarkGray
    Write-Host " cargo: $cargoV" -ForegroundColor DarkGray
} catch { Fail 'Rust toolchain not available' }

# 2. Formatting check
Write-Host '[2/9] cargo fmt -- --check' -ForegroundColor Yellow
$cargoFmt = & cargo fmt --all -- --check 2>&1
if ($LASTEXITCODE -ne 0) { Write-Host $cargoFmt; Fail 'Formatting issues (run cargo fmt --all)' } else { Write-Host ' OK' -ForegroundColor Green }

# 3. Clippy (warnings as errors)
Write-Host '[3/9] cargo clippy' -ForegroundColor Yellow
& cargo clippy --workspace --all-targets --all-features -- -D warnings
if ($LASTEXITCODE -ne 0) { Fail 'Clippy failed' } else { Write-Host ' OK' -ForegroundColor Green }

# 4. Build (debug)
Write-Host '[4/9] cargo build --workspace' -ForegroundColor Yellow
& cargo build --workspace
if ($LASTEXITCODE -ne 0) { Fail 'Build failed' } else { Write-Host ' OK' -ForegroundColor Green }

# 5. Tests
Write-Host '[5/9] cargo test --workspace' -ForegroundColor Yellow
& cargo test --workspace --all-features --quiet
if ($LASTEXITCODE -ne 0) { Fail 'Tests failed' } else { Write-Host ' OK' -ForegroundColor Green }

# 6. License header check
Write-Host '[6/9] License headers' -ForegroundColor Yellow
$missing = @()
Get-ChildItem -Path . -Recurse -Include *.rs | Where-Object { $_.FullName -notmatch '\\target\\' } | ForEach-Object {
    $firstLine = Get-Content $_.FullName -TotalCount 1
    if ($firstLine -notmatch '^// Copyright 2025 eraflo') { $missing += $_.FullName }
}
if ($missing.Count -gt 0) {
    Write-Host ' Missing headers in:' -ForegroundColor Red
    $missing | ForEach-Object { Write-Host "  $_" -ForegroundColor Red }
    Fail 'License header check failed'
} else { Write-Host ' OK' -ForegroundColor Green }

# 7. Dependency vulnerability audit (cargo-audit)
Write-Host '[7/9] cargo audit (vulnerabilities)' -ForegroundColor Yellow
$auditCmd = Get-Command cargo-audit -ErrorAction SilentlyContinue
if (-not $auditCmd) {
    if ($Install) {
        Write-Host ' Installing cargo-audit...' -ForegroundColor DarkGray
        cargo install cargo-audit | Out-Null
        $auditCmd = Get-Command cargo-audit -ErrorAction SilentlyContinue
    }
}
if ($auditCmd) {
    cargo audit -q
    if ($LASTEXITCODE -ne 0) {
        if ($Full) { Fail 'cargo audit reported issues' } else { Write-Host ' WARN (issues found, not failing without --full)' -ForegroundColor Yellow }
    } else { Write-Host ' OK' -ForegroundColor Green }
} else {
    if ($Full) { Fail 'cargo-audit missing (install with --install-tools)' } else { Write-Host ' Skipped (cargo-audit not installed)' -ForegroundColor DarkYellow }
}

# 8. cargo-deny (licenses / bans)
Write-Host '[8/9] cargo deny check' -ForegroundColor Yellow
$denyCmd = Get-Command cargo-deny -ErrorAction SilentlyContinue
if (-not $denyCmd) {
    if ($Install) {
        Write-Host ' Installing cargo-deny...' -ForegroundColor DarkGray
        cargo install cargo-deny | Out-Null
        $denyCmd = Get-Command cargo-deny -ErrorAction SilentlyContinue
    }
}
if ($denyCmd) {
    cargo deny check -q
    if ($LASTEXITCODE -ne 0) {
        if ($Full) { Fail 'cargo-deny reported issues' } else { Write-Host ' WARN (deny issues, not failing without --full)' -ForegroundColor Yellow }
    } else { Write-Host ' OK' -ForegroundColor Green }
} else {
    if ($Full) { Fail 'cargo-deny missing (install with --install-tools)' } else { Write-Host ' Skipped (cargo-deny not installed)' -ForegroundColor DarkYellow }
}

# 9. Binary size (release build)
Write-Host '[9/9] Binary size (release build)' -ForegroundColor Yellow
& cargo build --workspace --release -q
if ($LASTEXITCODE -ne 0) { Fail 'Release build failed' }
$sizes = @()
if (Test-Path 'target/release/sandbox.exe') { $sizes += Get-Item 'target/release/sandbox.exe' }
Get-ChildItem 'target/release' -Filter 'libkhora_engine_core*' | ForEach-Object { $sizes += $_ }
if ($sizes.Count -gt 0) {
    foreach ($s in $sizes) { Write-Host ("  {0} - {1} KB" -f $s.Name, [math]::Round($s.Length/1KB,1)) -ForegroundColor DarkGray }
    Write-Host ' OK' -ForegroundColor Green
} else { Write-Host ' No artifacts found' -ForegroundColor Yellow }

$stopwatch.Stop()
Write-Host "=== SUCCESS: All checks passed in $([math]::Round($stopwatch.Elapsed.TotalSeconds,2))s ===" -ForegroundColor Green
Write-Host 'Hints: use --install-tools to auto-install audit/deny; --full to fail on security warnings.' -ForegroundColor DarkGray

# Optional next suggestions
Write-Host 'Next: git add . ; git commit -m "feat: GPU timestamp hooks + tests" ; git push' -ForegroundColor DarkGray

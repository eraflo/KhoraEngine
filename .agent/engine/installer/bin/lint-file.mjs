#!/usr/bin/env node
// lint-file — runs `cargo xtask lint-file` on Rust files, for the hooks.
//
//   lint-file.mjs [<path>]   Claude Code PostToolUse hook: the path, else the
//                            hook's stdin JSON (`tool_input.file_path`). Exits 2
//                            with the findings on stderr, which Claude Code
//                            feeds back to the agent; 0 for a clean file or any
//                            file lint-file does not read.
//   lint-file.mjs --staged   pre-commit: the staged `.rs` files. Exits 1 on a
//                            finding, which blocks the commit.
//
// When lint-file cannot run (xtask fails to build, say), it says so and exits 1
// from the hook (shown to the user, not blocking) and 0 from the pre-commit.

import fs from 'node:fs';
import { makeContext, stagedFiles } from '../lib/core.mjs';
import { hookPath, lintablePaths, runLint, FINDINGS } from '../lib/lint.mjs';

async function main() {
  const ctx = makeContext();
  const args = process.argv.slice(2);

  if (args.includes('--staged')) {
    const files = lintablePaths(ctx.repoRoot, await stagedFiles(ctx));
    const { status, output } = runLint(ctx.repoRoot, files);
    if (status === null) {
      console.error(`lint-file skipped: ${output}`);
      return 0;
    }
    if (status === FINDINGS) {
      console.error(output.trimEnd());
      console.error('\nCommit blocked by `cargo xtask lint-file`: fix the findings above.');
      return 1;
    }
    return 0;
  }

  const stdin = args.some((a) => a.trim() !== '' && !a.startsWith('--')) || process.stdin.isTTY
    ? ''
    : readStdin();
  const file = hookPath(args, stdin);
  const files = lintablePaths(ctx.repoRoot, file ? [file] : []);
  const { status, output } = runLint(ctx.repoRoot, files);
  if (status === null) {
    console.error(output);
    return 1;
  }
  if (status === FINDINGS) {
    console.error(output.trimEnd());
    return 2;
  }
  return 0;
}

function readStdin() {
  try { return fs.readFileSync(0, 'utf8'); } catch { return ''; }
}

main().then((code) => process.exit(code), (e) => {
  console.error(`lint-file: ${e.stack || e.message}`);
  process.exit(1);
});

// Tests for lib/lint.mjs: how the lint-file hook finds the written path, which
// files it hands to `cargo xtask lint-file`, which xtask it runs, and what it
// makes of each exit code.

import { test, describe } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import { hookPath, lintablePaths, lintCommand, runLint, CLEAN, FINDINGS } from './lint.mjs';

function tmpRepo(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'khora-lint-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true, maxRetries: 5 }));
  return root;
}

function touch(file, mtime) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, '');
  fs.utimesSync(file, mtime, mtime);
}

describe('hookPath — the path the hook lints', () => {
  test('a non-empty argument wins', () => {
    assert.equal(hookPath(['crates/a/src/lib.rs'], '{"tool_input":{"file_path":"other.rs"}}'), 'crates/a/src/lib.rs');
  });

  test('an empty argument falls back to tool_input.file_path on stdin', () => {
    const stdin = JSON.stringify({ tool_name: 'Edit', tool_input: { file_path: 'D:\\repo\\crates\\a\\src\\lib.rs' } });
    assert.equal(hookPath([''], stdin), 'D:\\repo\\crates\\a\\src\\lib.rs');
  });

  test('no argument and no usable stdin give no path', () => {
    assert.equal(hookPath([], ''), null);
    assert.equal(hookPath([''], 'not json'), null);
    assert.equal(hookPath([], '{"tool_input":{}}'), null);
  });
});

describe('lintablePaths — the files lint-file reads', () => {
  test('keeps .rs files under crates/, xtask/ and examples/, repo-relative and once', (t) => {
    const root = tmpRepo(t);
    const files = lintablePaths(root, [
      path.join(root, 'crates', 'a', 'src', 'lib.rs'),
      'xtask/src/main.rs',
      'crates/a/src/lib.rs',
      'examples/sandbox/src/main.rs',
    ]);
    assert.deepEqual(files, ['crates/a/src/lib.rs', 'xtask/src/main.rs', 'examples/sandbox/src/main.rs']);
  });

  test('drops other files, other folders and paths outside the repository', (t) => {
    const root = tmpRepo(t);
    const outside = path.join(path.dirname(root), 'elsewhere', 'crates', 'x.rs');
    assert.deepEqual(lintablePaths(root, ['docs/src/x.md', 'crates/a/Cargo.toml', 'build.rs', outside, null]), []);
  });
});

describe('lintCommand — the prebuilt xtask when fresh, else cargo', () => {
  const exe = process.platform === 'win32' ? 'xtask.exe' : 'xtask';

  test('a binary newer than every xtask source runs directly', (t) => {
    const root = tmpRepo(t);
    touch(path.join(root, 'xtask', 'src', 'main.rs'), new Date(1000_000));
    touch(path.join(root, 'xtask', 'Cargo.toml'), new Date(1000_000));
    touch(path.join(root, 'target', 'debug', exe), new Date(2000_000));
    const { cmd, args, cargo } = lintCommand(root);
    assert.equal(cargo, false);
    assert.equal(cmd, path.join(root, 'target', 'debug', exe));
    assert.deepEqual(args, ['lint-file']);
  });

  test('a source changed after the build, or no binary, goes through cargo', (t) => {
    const root = tmpRepo(t);
    touch(path.join(root, 'xtask', 'src', 'lint', 'order.rs'), new Date(3000_000));
    touch(path.join(root, 'target', 'debug', exe), new Date(2000_000));
    assert.deepEqual(lintCommand(root), { cmd: 'cargo', args: ['xtask', 'lint-file'], cargo: true });
    fs.rmSync(path.join(root, 'target'), { recursive: true });
    assert.equal(lintCommand(root).cargo, true);
  });
});

describe('runLint — exit codes', () => {
  const fake = (status, stdout = '', stderr = '') => {
    const calls = [];
    const run = (cmd, args, opts) => { calls.push({ cmd, args, opts }); return { status, stdout, stderr }; };
    return { run, calls };
  };

  test('no file runs nothing and is clean', (t) => {
    const { run, calls } = fake(FINDINGS);
    assert.deepEqual(runLint(tmpRepo(t), [], { run }), { status: CLEAN, output: '' });
    assert.equal(calls.length, 0);
  });

  test('exit 1 is a finding, with lint-file\'s report', (t) => {
    const report = 'crates/a/src/lib.rs:3: constants_come_before_the_code: `const A` after `fn f` (line 1)\n';
    const { run, calls } = fake(FINDINGS, report);
    const r = runLint(tmpRepo(t), ['crates/a/src/lib.rs'], { run });
    assert.equal(r.status, FINDINGS);
    assert.equal(r.output, report);
    assert.deepEqual(calls[0].args.slice(-1), ['crates/a/src/lib.rs']);
  });

  test('any other exit means lint-file could not run', (t) => {
    const { run } = fake(101, '', 'error: could not compile `xtask`');
    const r = runLint(tmpRepo(t), ['crates/a/src/lib.rs'], { run });
    assert.equal(r.status, null);
    assert.match(r.output, /could not compile/);
  });

  test('cargo builds into the default target: CARGO_TARGET_DIR is dropped', (t) => {
    const before = process.env.CARGO_TARGET_DIR;
    process.env.CARGO_TARGET_DIR = 'elsewhere';
    t.after(() => { if (before === undefined) delete process.env.CARGO_TARGET_DIR; else process.env.CARGO_TARGET_DIR = before; });
    const { run, calls } = fake(CLEAN);
    runLint(tmpRepo(t), ['crates/a/src/lib.rs'], { run });
    assert.equal(calls[0].cmd, 'cargo');
    assert.equal(calls[0].opts.env.CARGO_TARGET_DIR, undefined);
  });

  test('many files are linted in chunks, a finding in any chunk counts', (t) => {
    let call = 0;
    const run = (cmd, args) => ({ status: call++ === 1 ? FINDINGS : CLEAN, stdout: `${args.length}\n` });
    const files = Array.from({ length: 150 }, (_, i) => `crates/a/src/f${i}.rs`);
    const r = runLint(tmpRepo(t), files, { run });
    assert.equal(call, 2);
    assert.equal(r.status, FINDINGS);
  });
});

// Zero-dependency secret scanner for the pre-commit hook.
// Scans staged (or given) text files for high-confidence secret patterns and
// reports matches. A line carrying `khora-ai:allow-secret` is allowlisted.

import fs from 'node:fs';
import path from 'node:path';
import { exists, stagedFiles, log } from './core.mjs';

const RULES = [
  ['private-key', /-----BEGIN (?:RSA |EC |OPENSSH |DSA |PGP )?PRIVATE KEY-----/],
  ['aws-access-key', /\bAKIA[0-9A-Z]{16}\b/],
  ['aws-secret', /aws_secret_access_key\s*[=:]\s*['"][A-Za-z0-9/+]{40}['"]/i],
  ['gcp-service-account', /"type"\s*:\s*"service_account"/],
  ['github-token', /\b(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9]{36,}\b|\bgithub_pat_[A-Za-z0-9_]{40,}\b/],
  ['slack-token', /\bxox[baprs]-[A-Za-z0-9-]{10,}\b/],
  ['google-api-key', /\bAIza[0-9A-Za-z_\-]{35}\b/],
  ['generic-secret', /\b(?:password|passwd|secret|api[_-]?key|access[_-]?token|auth[_-]?token)\b\s*[=:]\s*['"][^'"\s]{8,}['"]/i],
  ['bearer', /\bAuthorization\s*:\s*Bearer\s+[A-Za-z0-9._\-]{20,}/i],
];

const NAME_RULES = [
  ['env-file', /(^|\/)\.env(\.[\w-]+)?$/],
  ['key-file', /\.(pem|key|p12|pfx)$/i],
  ['ssh-key', /(^|\/)id_(rsa|ed25519|ecdsa|dsa)$/],
];

const PLACEHOLDER = /(test|fake|dummy|example|placeholder|your[_-]?|xxx|changeme|not[_-]?real|<.*>)/i;
const SKIP_DIR = /(^|\/)(\.git|node_modules|target|\.dist)\//;

function isProbablyBinary(buf) {
  const n = Math.min(buf.length, 8000);
  for (let i = 0; i < n; i++) if (buf[i] === 0) return true;
  return false;
}

export async function scanSecrets(ctx, { staged }) {
  const files = staged ? await stagedFiles(ctx) : [];
  const findings = [];

  for (const relPath of files) {
    if (SKIP_DIR.test('/' + relPath)) continue;
    for (const [rule, re] of NAME_RULES) {
      if (re.test(relPath)) findings.push({ file: relPath, line: 0, rule });
    }
    const abs = path.join(ctx.repoRoot, relPath);
    if (!exists(abs)) continue;
    let buf;
    try { buf = fs.readFileSync(abs); } catch { continue; }
    if (isProbablyBinary(buf)) continue;
    const text = buf.toString('utf8');
    // A real GCP service-account JSON always carries an actual PEM private-key
    // block. Docs that merely *describe* the pattern (like this installer's own
    // security docs) don't — so only flag the service_account marker when a real
    // key block is present in the same file. Prevents self-tripping on prose.
    const hasPrivateKeyBlock = /-----BEGIN (?:[A-Z0-9 ]+ )?PRIVATE KEY-----/.test(text);
    const lines = text.split('\n');
    lines.forEach((line, i) => {
      if (/khora-ai:allow-secret/.test(line)) return;
      for (const [rule, re] of RULES) {
        if (re.test(line)) {
          if (rule === 'generic-secret' && PLACEHOLDER.test(line)) continue;
          if (rule === 'gcp-service-account' && !hasPrivateKeyBlock) continue;
          findings.push({ file: relPath, line: i + 1, rule });
        }
      }
    });
  }

  if (findings.length) {
    log.err(`secret-scan blocked the commit — ${findings.length} potential secret(s):`);
    for (const f of findings) {
      console.error(`    ${f.file}${f.line ? ':' + f.line : ''}  [${f.rule}]`);
    }
    console.error('  Remove the secret (use env vars / an ignored local file). If it was ever committed,');
    console.error('  ROTATE it. False positive? append "# khora-ai:allow-secret" to the line.');
    return false;
  }
  return true;
}

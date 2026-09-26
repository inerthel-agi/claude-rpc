#!/usr/bin/env node
'use strict';

// Copies the release executable to bin/claude-rpc.exe (Windows only).

const fs = require('node:fs');
const path = require('node:path');

const ROOT = path.resolve(__dirname, '..');
const source = path.join(ROOT, 'src-tauri', 'target', 'release', 'claude-rpc.exe');
const target = path.join(ROOT, 'bin', 'claude-rpc.exe');

if (process.platform !== 'win32') {
  console.error('[export-tauri] Claude RPC only builds on Windows');
  process.exit(1);
}
if (!fs.existsSync(source)) {
  console.error(`[export-tauri] missing ${path.relative(ROOT, source)}`);
  process.exit(1);
}

fs.mkdirSync(path.dirname(target), { recursive: true });
fs.copyFileSync(source, target);
console.log(`[export-tauri] ${path.relative(ROOT, target)}`);

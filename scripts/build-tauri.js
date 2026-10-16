#!/usr/bin/env node
'use strict';

const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

const ROOT = path.resolve(__dirname, '..');

function run(command, args) {
  // Quote the command to handle spaces in paths (e.g. C:\Program Files\nodejs\node.exe)
  const res = spawnSync(`"${command}"`, args, {
    cwd: ROOT,
    stdio: 'inherit',
    shell: true,
  });
  if (res.error) {
    console.error(`Failed to run ${command}: ${res.error.message}`);
  }
  if (res.status !== 0) process.exit(res.status ?? 1);
}

function tauriBin() {
  return path.join(
    ROOT,
    'node_modules',
    '.bin',
    'tauri.cmd',
  );
}

if (process.platform !== 'win32') {
  console.error('Claude RPC only builds on Windows.');
  process.exit(1);
}

const args = ['build', '--bundles', 'nsis'];
if (!process.env.TAURI_SIGNING_PRIVATE_KEY) {
  // Without the updater key, skip the .sig instead of failing after the
  // installer is built. The release workflow sets the key and checks the .sig.
  const override = path.join(os.tmpdir(), 'claude-rpc-unsigned.conf.json');
  fs.writeFileSync(override, JSON.stringify({ bundle: { createUpdaterArtifacts: false } }));
  console.warn('[build] TAURI_SIGNING_PRIVATE_KEY is not set: building without the updater signature.');
  args.push('--config', `"${override}"`);
}

run(tauriBin(), args);
run(process.execPath, [path.join(ROOT, 'scripts', 'export-tauri-binary.js')]);

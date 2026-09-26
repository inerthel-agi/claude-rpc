#!/usr/bin/env node
'use strict';

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

run(tauriBin(), ['build', '--bundles', 'nsis']);
run(process.execPath, [path.join(ROOT, 'scripts', 'export-tauri-binary.js')]);

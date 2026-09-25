#!/usr/bin/env node
'use strict';

const { spawnSync } = require('node:child_process');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

// Minimum patched versions for advisories found in the locked dependency graph.
const minimumSafe = {
  anyhow: '1.0.103',
  'event-listener': '5.4.2',
  'quick-xml': '0.41.0',
  rustls: '0.23.45',
  serde_with: '3.21.0',
  tar: '0.4.46',
  tauri: '2.11.1',
};
const lock = fs.readFileSync(path.join(__dirname, '..', 'src-tauri', 'Cargo.lock'), 'utf8');
const packages = lock.split('[[package]]').slice(1).map((entry) => ({
  name: entry.match(/^name = "([^"]+)"/m)?.[1],
  version: entry.match(/^version = "([^"]+)"/m)?.[1],
}));
const compare = (left, right) => {
  const a = left.split('.').map(Number);
  const b = right.split('.').map(Number);
  for (let i = 0; i < 3; i += 1) {
    if (a[i] !== b[i]) return a[i] - b[i];
  }
  return 0;
};
for (const [name, minimum] of Object.entries(minimumSafe)) {
  const versions = packages.filter((entry) => entry.name === name).map((entry) => entry.version);
  assert(versions.length > 0, `${name} missing from Cargo.lock`);
  assert(versions.every((version) => compare(version, minimum) >= 0),
    `${name} must be at least ${minimum}; locked: ${versions.join(', ')}`);
}

const cargo = process.env.CARGO
  || (process.platform === 'win32'
    ? path.join(process.env.USERPROFILE || '', '.cargo', 'bin', 'cargo.exe')
    : 'cargo');

const result = spawnSync(cargo, ['test', '--manifest-path', 'src-tauri/Cargo.toml'], {
  stdio: 'inherit',
});

process.exit(result.status ?? 1);

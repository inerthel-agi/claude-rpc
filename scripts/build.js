#!/usr/bin/env node
'use strict';

// Builds the release binary, copies it to bin/claude-rpc.exe and packages the
// NSIS installer. With TAURI_SIGNING_PRIVATE_KEY set, it also writes the
// installer's .sig, which installs of 4.0.x check before they update.

const fs = require('node:fs');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

const ROOT = path.resolve(__dirname, '..');
const RELEASE = path.join(ROOT, 'src-tauri', 'target', 'release');
const BUNDLE = path.join(RELEASE, 'bundle', 'nsis');
// The version lives in Cargo.toml only, so the app and its installer always agree.
const version = /^version\s*=\s*"([^"]+)"/m.exec(
  fs.readFileSync(path.join(ROOT, 'src-tauri', 'Cargo.toml'), 'utf8'),
)[1];
// Signs like the Tauri 2 bundler did, with the same key format.
const SIGNER = '@tauri-apps/cli@2.10.1';

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { cwd: ROOT, stdio: 'inherit', ...options });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}

function findMakensis() {
  const candidates = [
    'makensis',
    path.join(process.env['ProgramFiles(x86)'] || '', 'NSIS', 'makensis.exe'),
    path.join(process.env.LOCALAPPDATA || '', 'tauri', 'NSIS', 'makensis.exe'),
  ];
  return candidates.find((candidate) => spawnSync(candidate, ['/VERSION']).status === 0);
}

if (process.platform !== 'win32') {
  console.error('Claude RPC only builds on Windows.');
  process.exit(1);
}

run('cargo', ['build', '--release', '--locked', '--manifest-path', path.join(ROOT, 'src-tauri', 'Cargo.toml')]);

const exe = path.join(RELEASE, 'claude-rpc.exe');
const portable = path.join(ROOT, 'bin', 'claude-rpc.exe');
fs.mkdirSync(path.dirname(portable), { recursive: true });
fs.copyFileSync(exe, portable);
console.log(`[build] ${path.relative(ROOT, portable)}`);

const makensis = findMakensis();
if (!makensis) {
  console.error('[build] NSIS (makensis) not found: the installer was not built.');
  process.exit(1);
}
const setup = path.join(BUNDLE, `Claude RPC_${version}_x64-setup.exe`);
fs.mkdirSync(BUNDLE, { recursive: true });
run(makensis, [
  '/V2',
  `/DVERSION=${version}`,
  `/DSOURCE=${exe}`,
  `/DOUTFILE=${setup}`,
  path.join(ROOT, 'installer', 'windows.nsi'),
]);
console.log(`[build] ${path.relative(ROOT, setup)}`);

const signature = `${setup}.sig`;
fs.rmSync(signature, { force: true });
if (!process.env.TAURI_SIGNING_PRIVATE_KEY) {
  console.warn('[build] TAURI_SIGNING_PRIVATE_KEY is not set: no updater signature (.sig).');
} else {
  // The key and its password are read from the environment, never from the command line.
  // npx needs a shell on Windows; the installer path has a space.
  run('npx', ['--yes', SIGNER, 'signer', 'sign', `"${setup}"`], { shell: true });
  if (!fs.existsSync(signature)) {
    console.error('[build] the updater signature was not written.');
    process.exit(1);
  }
  console.log(`[build] ${path.relative(ROOT, signature)}`);
}

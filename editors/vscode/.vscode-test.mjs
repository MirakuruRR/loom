// The integration tests: VS Code, with this extension, opens a workspace whose folders are two
// Unreal-like projects, made fresh for each run as packages/loom-cli/tests/lsp.rs makes one. Every
// project runs the `loom` that `cargo build -p loom-cli` builds, which the workspace's settings name.

import { copyFileSync, existsSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';
import { defineConfig } from '@vscode/test-cli';

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '../..');
const loom = join(repo, 'target/debug', process.platform === 'win32' ? 'loom.exe' : 'loom');
if (!existsSync(loom)) {
  throw new Error(`${loom} is missing: build it with cargo build -p loom-cli`);
}

const workspace = join(here, '.vscode-test/workspace');
rmSync(workspace, { recursive: true, force: true });

/** A project in the workspace, with these sources by their paths under Content. */
function project(name, sources) {
  const root = join(workspace, name);
  mkdirSync(join(root, 'Intermediate/Loom'), { recursive: true });
  writeFileSync(join(root, `${name}.uproject`), '{}');
  copyFileSync(join(repo, 'packages/loom/tests/types/small.json'), join(root, 'Intermediate/Loom/types.json'));
  for (const [path, text] of Object.entries(sources)) {
    const file = join(root, 'Content', path);
    mkdirSync(dirname(file), { recursive: true });
    writeFileSync(file, text);
  }
}

project('Alpha', {
  'X/BP_A.lm':
    'blueprint BP_A : Actor at /Game/X/BP_A\n\n// How many times it was bumped.\nvar Count: int\n\nfn Bump() {\n    Count = Count + 1\n}\n',
  'X/BP_B.lm': 'blueprint BP_B : Actor at /Game/X/BP_B\nvar Other: BP_A\n\nfn Poke() {\n    Other.Bump()\n}\n',
});
project('Beta', {
  'Y/BP_C.lm': 'blueprint BP_C : Actor at /Game/Y/BP_C\n\nvar Other: BP_A\n',
});
const workspaceFile = join(workspace, 'loom.code-workspace');
writeFileSync(
  workspaceFile,
  JSON.stringify(
    { folders: [{ path: 'Alpha' }, { path: 'Beta' }], settings: { 'loom.server.path': loom, 'loom.trace.server': 'messages' } },
    null,
    2,
  ),
);

export default defineConfig({
  files: 'out/test/integration/**/*.test.js',
  workspaceFolder: workspaceFile,
  // The Loom output logs every message to and from the servers, in .vscode-test/user-data/logs.
  launchArgs: ['--disable-extensions', '--disable-workspace-trust', '--disable-gpu', '--log', 'pierrekin.loom:trace'],
  mocha: { ui: 'bdd', timeout: 60_000 },
});

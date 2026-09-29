// Builds the extension into dist/: its code, bundled with what it depends on, so the package
// ships no node_modules, and a copy of the repository's TextMate grammar, which the site
// highlights with too. It also copies the repository's license beside package.json, where vsce
// looks for one. Both copies are ignored by git, so each has one master copy.
//
//   node esbuild.mjs               a build to debug
//   node esbuild.mjs --production  a minified build, as the package holds
//   node esbuild.mjs --watch       rebuilds on each change

import { copyFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';

const here = dirname(fileURLToPath(import.meta.url));
const production = process.argv.includes('--production');
const watch = process.argv.includes('--watch');

mkdirSync(join(here, 'dist'), { recursive: true });
copyFileSync(join(here, '../textmate/loom.tmLanguage.json'), join(here, 'dist/loom.tmLanguage.json'));
copyFileSync(join(here, '../../LICENSE.txt'), join(here, 'LICENSE.txt'));

const context = await esbuild.context({
  entryPoints: [join(here, 'src/extension.ts')],
  outfile: join(here, 'dist/extension.js'),
  bundle: true,
  format: 'cjs',
  platform: 'node',
  target: 'node20',
  external: ['vscode'],
  minify: production,
  sourcemap: !production,
  sourcesContent: false,
  logLevel: 'info',
});

if (watch) {
  await context.watch();
} else {
  await context.rebuild();
  await context.dispose();
}

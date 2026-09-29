import * as assert from 'node:assert/strict';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { findProgram, PLUGIN_PROGRAMS } from '../../discovery';

describe('finding the loom to run', () => {
  let dir: string;

  beforeEach(() => {
    dir = fs.mkdtempSync(path.join(os.tmpdir(), 'loom-vscode-discovery-'));
  });

  afterEach(() => {
    fs.rmSync(dir, { recursive: true, force: true });
  });

  /** A file with this text, and the folders it is in. */
  function write(file: string, text = '', mode = 0o644): string {
    const full = path.join(dir, file);
    fs.mkdirSync(path.dirname(full), { recursive: true });
    fs.writeFileSync(full, text, { mode });
    return full;
  }

  it("runs the loom in the project's plugin for each platform", () => {
    const root = path.join(dir, 'Game');
    for (const [platform, program] of [
      ['linux', 'Linux/loom'],
      ['darwin', 'Mac/loom'],
      ['win32', 'Win64/loom.exe'],
    ] as const) {
      const expected = write(path.join('Game', PLUGIN_PROGRAMS, program), '', 0o755);
      assert.deepEqual(findProgram(root, undefined, platform), { ok: true, path: expected }, platform);
    }
  });

  it('says where it looked when the plugin has no loom', () => {
    write('Game/Game.uproject', '{}');
    const found = findProgram(path.join(dir, 'Game'), undefined, 'linux');
    assert.ok(!found.ok);
    const looked = path.join(dir, 'Game', PLUGIN_PROGRAMS, 'Linux', 'loom');
    assert.ok(found.message.includes(`at ${looked}.`), found.message);
    assert.ok(found.message.includes('loom.server.path'), found.message);
  });

  it('does not run a loom that cannot be run', function () {
    if (process.platform === 'win32') {
      this.skip();
    }
    write(path.join('Game', PLUGIN_PROGRAMS, 'Linux/loom'), '', 0o644);
    assert.ok(!findProgram(path.join(dir, 'Game'), undefined, 'linux').ok);
  });

  it('runs the loom the setting names, in place of the plugin', () => {
    write(path.join('Game', PLUGIN_PROGRAMS, 'Linux/loom'), '', 0o755);
    const other = write('Other/loom', '', 0o755);
    assert.deepEqual(findProgram(path.join(dir, 'Game'), other, 'linux'), { ok: true, path: other });
  });

  it('says the setting is wrong when what it names is missing', () => {
    const missing = path.join(dir, 'nowhere/loom');
    const found = findProgram(path.join(dir, 'Game'), missing, 'linux');
    assert.deepEqual(found, { ok: false, message: `loom.server.path names ${missing}, which is not a program that can be run.` });
  });
});

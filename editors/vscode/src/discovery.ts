// Finding the `loom` to run for a project. Nothing here needs VS Code, so it is tested on its own.

import * as fs from 'node:fs';
import * as path from 'node:path';

/** Where the plugin keeps the programs it runs, under a project's root. */
export const PLUGIN_PROGRAMS = path.join('Plugins', 'LoomEditor', 'Binaries', 'ThirdParty', 'Loom');

/** The setting that names another `loom` to run. */
export const PATH_SETTING = 'loom.server.path';

/** The plugin's folder for a platform and its program's name there, as the plugin runs it. */
function platformProgram(platform: NodeJS.Platform): string | undefined {
  switch (platform) {
    case 'win32':
      return path.join('Win64', 'loom.exe');
    case 'darwin':
      return path.join('Mac', 'loom');
    case 'linux':
      return path.join('Linux', 'loom');
    default:
      return undefined;
  }
}

export type Program = { ok: true; path: string } | { ok: false; message: string };

/**
 * The `loom` to run for a project: the one the setting names, or else the one in the project's
 * plugin. Either must be a program that can be run.
 */
export function findProgram(root: string, setting: string | undefined, platform = process.platform): Program {
  if (setting) {
    if (runnable(setting, platform)) {
      return { ok: true, path: setting };
    }
    return { ok: false, message: `${PATH_SETTING} names ${setting}, which is not a program that can be run.` };
  }
  const program = platformProgram(platform);
  if (program === undefined) {
    return { ok: false, message: `The LoomEditor plugin has no loom for ${platform}: set ${PATH_SETTING} to a loom to run.` };
  }
  const found = path.join(root, PLUGIN_PROGRAMS, program);
  if (runnable(found, platform)) {
    return { ok: true, path: found };
  }
  return {
    ok: false,
    message: `Found no loom to run at ${found}. Open the project's folder, the one with its .uproject, or set ${PATH_SETTING} to a loom to run.`,
  };
}

function runnable(file: string, platform: NodeJS.Platform): boolean {
  try {
    if (!fs.statSync(file).isFile()) {
      return false;
    }
    // Windows has no execute bit: a file is a program by its name.
    if (platform !== 'win32') {
      fs.accessSync(file, fs.constants.X_OK);
    }
    return true;
  } catch {
    return false;
  }
}

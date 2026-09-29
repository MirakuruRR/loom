// Runs `loom lsp` for Loom sources: for each folder open in VS Code that has a source open, the
// `loom` the plugin in that folder's project carries, which builds its sources, or the one
// `loom.server.path` names. Each folder is a project, the one with its .uproject.

import * as path from 'node:path';
import * as vscode from 'vscode';
import { LanguageClient } from 'vscode-languageclient/node';
import { findProgram, PATH_SETTING } from './discovery';

/** Each project's client, by its root, so that no project is built by another's `loom`. */
const clients = new Map<string, LanguageClient>();

/** What has been shown the user, so that each is said once until a restart. */
const told = new Set<string>();

let output: vscode.LogOutputChannel;

/** What the extension gives other extensions, and its tests. */
export interface Api {
  /** The roots of the projects a server is running for. */
  projects(): string[];
}

export function activate(context: vscode.ExtensionContext): Api {
  output = vscode.window.createOutputChannel('Loom', { log: true });
  context.subscriptions.push(
    output,
    vscode.workspace.onDidOpenTextDocument(opened),
    vscode.workspace.onDidGrantWorkspaceTrust(openAll),
    vscode.workspace.onDidChangeWorkspaceFolders((event) => {
      for (const folder of event.removed) {
        void stop(folder.uri.fsPath);
      }
    }),
    vscode.workspace.onDidChangeConfiguration((event) => {
      if (event.affectsConfiguration('loom.server')) {
        void restart();
      }
    }),
    vscode.commands.registerCommand('loom.restartServer', restart),
  );
  openAll();
  return {
    projects: () => [...clients].filter(([, client]) => client.isRunning()).map(([root]) => root),
  };
}

export function deactivate(): Promise<void> {
  return stopAll();
}

function openAll(): void {
  vscode.workspace.textDocuments.forEach(opened);
}

/** Starts the client for the folder a Loom source is in, unless it has one. */
function opened(document: vscode.TextDocument): void {
  if (document.languageId !== 'loom' || document.uri.scheme !== 'file') {
    return;
  }
  // The server is a program in the workspace, so it waits for the workspace to be trusted.
  if (!vscode.workspace.isTrusted) {
    return;
  }
  const folder = vscode.workspace.getWorkspaceFolder(document.uri);
  if (folder === undefined) {
    tell(
      'no folder',
      'warn',
      `${document.uri.fsPath} is not in a folder open in VS Code, so Loom's language server has no project to build. Open the project's folder, the one with its .uproject.`,
    );
    return;
  }
  const root = folder.uri.fsPath;
  if (!clients.has(root)) {
    void start(root);
  }
}

async function start(root: string): Promise<void> {
  const config = vscode.workspace.getConfiguration('loom.server', vscode.Uri.file(root));
  const setting = config.get<string>('path')?.trim();
  const program = findProgram(root, setting ? path.resolve(root, setting) : undefined);
  if (!program.ok) {
    tell(root, 'error', program.message);
    return;
  }
  const args = config.get<string[]>('args') ?? ['lsp'];
  output.info(`Running ${program.path} ${args.join(' ')} for ${root}`);
  const client = new LanguageClient(
    'loom',
    `Loom (${path.basename(root)})`,
    { command: program.path, args, options: { cwd: root } },
    {
      // Only this project's sources, as the path the editor has them by.
      documentSelector: [
        { scheme: 'file', language: 'loom', pattern: { baseUri: vscode.Uri.file(root).toString(), pattern: '**/*' } },
      ],
      outputChannel: output,
      // A source open but not yet shown when the server starts is sent once it is shown. Without
      // this, the client holds it back until it next sends the server something, and a source
      // opened alone gets no diagnostics.
      textSynchronization: { delayOpenNotifications: true },
    },
  );
  clients.set(root, client);
  try {
    await client.start();
  } catch (error) {
    clients.delete(root);
    tell(root, 'error', `Loom's language server for ${root} did not start: ${String(error)}`);
  }
}

async function stop(root: string): Promise<void> {
  const client = clients.get(root);
  clients.delete(root);
  await client?.dispose();
}

/** Says something in the log, and once in a notification. */
function tell(key: string, level: 'warn' | 'error', message: string): void {
  output[level](message);
  if (told.has(key)) {
    return;
  }
  told.add(key);
  const show = level === 'error' ? vscode.window.showErrorMessage : vscode.window.showWarningMessage;
  void show(message, 'Open Settings', 'Show Log').then((choice) => {
    if (choice === 'Open Settings') {
      void vscode.commands.executeCommand('workbench.action.openSettings', PATH_SETTING);
    } else if (choice === 'Show Log') {
      output.show();
    }
  });
}

async function stopAll(): Promise<void> {
  const stopping = [...clients.values()].map((client) => client.dispose());
  clients.clear();
  await Promise.allSettled(stopping);
}

/** Stops every server and starts one again for each folder with a source open. */
async function restart(): Promise<void> {
  await stopAll();
  told.clear();
  openAll();
}

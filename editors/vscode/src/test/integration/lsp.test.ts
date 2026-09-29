// VS Code running `loom lsp` through the extension, in the workspace .vscode-test.mjs makes: the
// folders Alpha and Beta, each a project.

import * as assert from 'node:assert/strict';
import * as path from 'node:path';
import * as vscode from 'vscode';
import type { Api } from '../../extension';

/** A project, as the workspace folder of that name. */
function project(name: string): string {
  const folder = vscode.workspace.workspaceFolders?.find((open) => open.name === name);
  assert.ok(folder, `the tests need the workspace .vscode-test.mjs makes, with the folder ${name}`);
  return folder.uri.fsPath;
}

function source(name: string, file: string): vscode.Uri {
  return vscode.Uri.file(path.join(project(name), file));
}

/** A document's diagnostics, once they are as wanted. */
function diagnostics(uri: vscode.Uri, wanted: (found: vscode.Diagnostic[]) => boolean): Promise<vscode.Diagnostic[]> {
  return new Promise((resolve, reject) => {
    const check = (): boolean => {
      const found = vscode.languages.getDiagnostics(uri);
      if (!wanted(found)) {
        return false;
      }
      listener.dispose();
      clearTimeout(timer);
      resolve(found);
      return true;
    };
    const listener = vscode.languages.onDidChangeDiagnostics((event) => {
      if (event.uris.some((changed) => changed.toString() === uri.toString())) {
        check();
      }
    });
    const timer = setTimeout(() => {
      listener.dispose();
      reject(new Error(`${uri.fsPath} never had the diagnostics wanted: ${JSON.stringify(vscode.languages.getDiagnostics(uri))}`));
    }, 30_000);
    check();
  });
}

/** Once something is so. */
async function until(done: () => boolean): Promise<void> {
  for (const started = Date.now(); !done(); ) {
    if (Date.now() - started > 30_000) {
      throw new Error('it never happened');
    }
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
}

function hoverText(hovers: vscode.Hover[]): string {
  return hovers
    .flatMap((hover) => hover.contents)
    .map((content) => {
      // The server's hovers are Markdown.
      assert.ok(content instanceof vscode.MarkdownString, JSON.stringify(content));
      return content.value;
    })
    .join('\n');
}

async function definition(uri: vscode.Uri, line: number, character: number): Promise<vscode.Location> {
  const found = await vscode.commands.executeCommand<(vscode.Location | vscode.LocationLink)[]>(
    'vscode.executeDefinitionProvider',
    uri,
    new vscode.Position(line, character),
  );
  assert.equal(found.length, 1, JSON.stringify(found));
  const [first] = found;
  assert.ok(first);
  return 'targetUri' in first ? new vscode.Location(first.targetUri, first.targetSelectionRange ?? first.targetRange) : first;
}

describe('the language server', () => {
  const a = source('Alpha', 'Content/X/BP_A.lm');
  const b = source('Alpha', 'Content/X/BP_B.lm');
  const c = source('Beta', 'Content/Y/BP_C.lm');

  it("reports errors in the editor's text as it is typed", async () => {
    const document = await vscode.workspace.openTextDocument(b);
    await vscode.window.showTextDocument(document);
    const edit = new vscode.WorkspaceEdit();
    edit.replace(b, new vscode.Range(1, 0, 1, 15), 'var Other BP_A');
    assert.ok(await vscode.workspace.applyEdit(edit));

    const [error] = await diagnostics(b, (found) => found.length > 0);
    assert.ok(error);
    assert.equal(error.range.start.line, 1);
    assert.equal(error.range.start.character, 10);
    assert.equal(error.severity, vscode.DiagnosticSeverity.Error);
    assert.ok(error.message.includes("expected ':'"), error.message);

    // Fixed, the error is cleared.
    const fix = new vscode.WorkspaceEdit();
    fix.replace(b, document.lineAt(1).range, 'var Other: BP_A');
    assert.ok(await vscode.workspace.applyEdit(fix));
    await diagnostics(b, (found) => found.length === 0);
  });

  it('shows what a name is', async () => {
    const hovers = await vscode.commands.executeCommand<vscode.Hover[]>(
      'vscode.executeHoverProvider',
      b,
      new vscode.Position(4, 11),
    );
    assert.ok(hoverText(hovers).includes('fn Bump()'), hoverText(hovers));
  });

  it('goes to a declaration in another source', async () => {
    const bump = await definition(b, 4, 11);
    assert.equal(bump.uri.fsPath, a.fsPath);
    assert.deepEqual([bump.range.start.line, bump.range.start.character], [5, 3]);

    const blueprint = await definition(b, 1, 12);
    assert.equal(blueprint.uri.fsPath, a.fsPath);
    assert.deepEqual([blueprint.range.start.line, blueprint.range.start.character], [0, 10]);
  });

  it('runs a server for each folder, and builds each alone', async () => {
    const extension = vscode.extensions.getExtension<Api>('pierrekin.loom');
    assert.ok(extension);
    const projects = [project('Alpha'), project('Beta')];

    // Opened but not shown, its folder's server starts. Shown only once it runs, the source is
    // still sent to it.
    const document = await vscode.workspace.openTextDocument(c);
    await until(() => extension.exports.projects().length === 2);
    assert.deepEqual(extension.exports.projects().sort(), projects);
    await vscode.window.showTextDocument(document);

    // Beta does not have Alpha's sources.
    const [error] = await diagnostics(c, (found) => found.length > 0);
    assert.ok(error);
    assert.equal(error.range.start.line, 2);
    assert.ok(error.message.includes("unknown type 'BP_A'"), error.message);
  });
});

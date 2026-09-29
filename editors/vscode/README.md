# Loom

Support for [Loom](https://loom.pierrekin.me), a language that compiles to Unreal Blueprints, in
`.lm` files:

- Highlighting.
- Errors as you type, from compiling the project's sources as the editor has them.
- Hover, showing what a name is.
- Go to definition, across the project's sources.

## Requirements

- An Unreal project with the LoomEditor plugin in its `Plugins` folder. The extension runs the
  `loom` the plugin carries, so it always matches the compiler the project builds with.
- The project built once in Unreal, which saves the types Loom checks sources against.
- A trusted workspace. The language server is a program in the project, so it does not run in
  Restricted Mode.

The project is the folder open in VS Code, the one with its `.uproject`. A workspace with more than
one folder runs a language server for each.

## Settings

- `loom.server.path`: a `loom` to run in place of the plugin's. A relative path is from the
  project's folder.
- `loom.server.args`: what it is run with, `["lsp"]` by default.
- `loom.trace.server`: logs the messages between VS Code and the server to the Loom output.

`Loom: Restart Language Server` restarts every server, for after the plugin is rebuilt.

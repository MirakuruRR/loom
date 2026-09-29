//! Runs `loom lsp` for Loom sources: the one Zed's settings name in `lsp.loom.binary.path`, else
//! the `loom` the project's plugin carries, which builds its sources, else a `loom` on the PATH.

use zed_extension_api::{self as zed, settings::LspSettings, Command, LanguageServerId, Os, Worktree};

/// The plugin's descriptor, which says the project has it.
const PLUGIN: &str = "Plugins/LoomEditor/LoomEditor.uplugin";

struct Loom;

impl zed::Extension for Loom {
    fn new() -> Self {
        Loom
    }

    fn language_server_command(&mut self, id: &LanguageServerId, worktree: &Worktree) -> zed::Result<Command> {
        let binary = LspSettings::for_worktree(id.as_ref(), worktree)?.binary;
        let (path, args, env) = match binary {
            Some(binary) => (binary.path, binary.arguments, binary.env),
            None => (None, None, None),
        };
        let command = match path {
            Some(path) => path,
            None => match plugin_program(worktree) {
                Some(path) => path,
                // `loom lsp` finds each source's project from the source itself, so one on the PATH
                // serves a folder that isn't the project's own, such as its Content.
                None => worktree.which("loom").ok_or_else(|| {
                    format!(
                        "found no loom to run: {} has no {PLUGIN}, and there is no loom on the PATH. Open an Unreal project with the LoomEditor plugin, put loom on the PATH, or set lsp.loom.binary.path in Zed's settings",
                        worktree.root_path()
                    )
                })?,
            },
        };
        Ok(Command {
            command,
            args: args.unwrap_or_else(|| vec!["lsp".to_string()]),
            env: env.map(|env| env.into_iter().collect()).unwrap_or_default(),
        })
    }
}

/// `loom` in the plugin of the project Zed has open, if it has one. An extension reads only inside
/// the folder Zed has open, so that folder must be the project's, the one with its .uproject.
fn plugin_program(worktree: &Worktree) -> Option<String> {
    let root = worktree.root_path();
    worktree.read_text_file(PLUGIN).ok()?;
    // The platform folders are those the plugin runs its programs from.
    let (os, _) = zed::current_platform();
    let file = match os {
        Os::Windows => "Win64/loom.exe",
        Os::Mac => "Mac/loom",
        Os::Linux => "Linux/loom",
    };
    let separator = if os == Os::Windows { "\\" } else { "/" };
    let path = format!("{root}/Plugins/LoomEditor/Binaries/ThirdParty/Loom/{file}");
    Some(path.replace('/', separator))
}

zed::register_extension!(Loom);

//! `loom-mcp`: Loom's MCP server, which an agent runs. It offers the reference, the project's types
//! and the check as tools, on stdin and stdout. With `--setup`, or run from a terminal, it says
//! instead how to add it to an agent.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use loom::{docs, project::Project};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_handler, tool_router, ServerHandler, ServiceExt};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Parser)]
#[command(about = "Loom's MCP server, which an agent runs. Run from a terminal, it says how to add it to one.")]
struct Cli {
    /// The project, or a folder in it. The current folder by default.
    #[arg(long, value_name = "DIR", default_value = ".")]
    project: PathBuf,
    /// Say how to add this server to an agent, and exit.
    #[arg(long)]
    setup: bool,
    /// Say it as JSON: the Claude Code command, and the text.
    #[arg(long, requires = "setup")]
    json: bool,
}

#[derive(Clone)]
struct Loom {
    /// The folder the project is found from.
    dir: PathBuf,
}

#[derive(Deserialize, JsonSchema)]
struct DocsArgs {
    /// The page to show, such as `members`. Leave it out to list the pages.
    page: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct SearchArgs {
    /// Words every section found must mention, such as `dispatcher bind`.
    words: String,
}

#[derive(Deserialize, JsonSchema)]
struct TypesArgs {
    /// A class, struct or enum by name, such as `Actor`, or a function, as `Actor.K2_DestroyActor`.
    name: String,
}

#[tool_router]
impl Loom {
    /// The Loom language reference, for the version of Loom installed: its pages, or one page.
    #[tool]
    fn docs(&self, Parameters(args): Parameters<DocsArgs>) -> Result<String, String> {
        match args.page {
            Some(page) => docs::page(&page).map_err(|e| e.to_string()),
            None => Ok(docs::list()),
        }
    }

    /// Finds the sections of the Loom reference that mention some words.
    #[tool]
    fn docs_search(&self, Parameters(args): Parameters<SearchArgs>) -> String {
        docs::search(&args.words)
    }

    /// Looks up a class, struct, enum or function in the project's types, as the plugin last saw
    /// them. The project's own Loom Blueprints are not among them.
    #[tool]
    fn types(&self, Parameters(args): Parameters<TypesArgs>) -> Result<String, String> {
        Project::find(&self.dir).and_then(|p| p.types(&args.name)).map_err(|e| format!("{e:#}"))
    }

    /// Compiles every Loom source in the project without Unreal, and says what is wrong and where.
    /// A clean check does not build the Blueprints; saving a source, with the editor open, does.
    #[tool]
    fn check(&self) -> Result<String, String> {
        Project::find(&self.dir).and_then(|p| p.check()).map(|c| c.text()).map_err(|e| format!("{e:#}"))
    }
}

#[tool_handler(name = "loom")]
impl ServerHandler for Loom {}

/// How to add this server to an agent, for one project.
#[derive(Serialize)]
struct Setup {
    /// The command that adds it to Claude Code.
    claude: String,
    /// All of it, as text.
    text: String,
}

/// A path as a person types it: Windows' `\\?\` prefix, which canonical paths carry, is left off.
fn plain(path: &Path) -> String {
    let text = path.display().to_string();
    text.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(text)
}

/// This program's path, as it was run: through the plugin's folder, rather than wherever a link to
/// it leads.
fn own_path() -> Result<PathBuf> {
    match std::env::args_os().next().map(PathBuf::from) {
        Some(path) if path.components().count() > 1 => Ok(std::path::absolute(path)?),
        _ => Ok(std::env::current_exe()?),
    }
}

fn setup(from: &Path) -> Result<Setup> {
    let project = Project::find(from)
        .context("to say how to add it, loom-mcp needs the project: run it from the project's folder, or with --project")?;
    let (exe, project) = (plain(&own_path()?), plain(&project.root));
    let claude = format!(r#"claude mcp add loom -- "{exe}" --project "{project}""#);
    let config = serde_json::json!({ "mcpServers": { "loom": { "command": exe, "args": ["--project", project] } } });
    let text = format!(
        "loom-mcp is Loom's MCP server. An agent runs it to read the Loom reference, look up the\n\
         project's types and check its sources.\n\n\
         To add it to Claude Code, run this:\n\n    {claude}\n\n\
         For another agent, add it to the agent's MCP config:\n\n{}\n",
        serde_json::to_string_pretty(&config)?.lines().map(|l| format!("    {l}\n")).collect::<String>()
    );
    Ok(Setup { claude, text })
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if cli.setup || std::io::stdin().is_terminal() {
        let setup = setup(&cli.project)?;
        match cli.json {
            true => println!("{}", serde_json::to_string_pretty(&setup)?),
            false => print!("{}", setup.text),
        }
        return Ok(());
    }
    let server = Loom { dir: std::path::absolute(&cli.project)? };
    tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(async {
        server.serve(rmcp::transport::stdio()).await?.waiting().await?;
        Ok(())
    })
}

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use loom::{docs, ops, project};

mod lsp;

#[derive(Parser)]
#[command(about = "Compile loom source to Unreal Blueprints")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show the Loom reference: its pages, one page, or `search <words>` for where something is
    /// described.
    Docs {
        /// A page, or `search` followed by words.
        #[arg(num_args = 0..)]
        args: Vec<String>,
    },
    /// Look up a class, struct, enum or function, as `Class.Function`, in the project's types.
    Types {
        name: String,
        #[command(flatten)]
        project: ProjectArg,
    },
    /// List the project's sources, each with the Blueprint or enum it builds, and as JSON the
    /// folders it uses.
    Sources {
        #[command(flatten)]
        project: ProjectArg,
        /// Print them as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Compile the project's sources without Unreal, and say what is wrong and where.
    Check {
        #[command(flatten)]
        project: ProjectArg,
        /// Print the result as JSON, with the fields of the plugin's report.
        #[arg(long)]
        json: bool,
        /// Check an ops.json instead.
        #[arg(long, value_name = "OPS")]
        ops: Option<PathBuf>,
    },
    /// Compile the project's sources, each a Blueprint or an enum, to an ops.json each, against the
    /// types the plugin saved.
    Build {
        /// Where to write <Blueprint>.json for each, and build.json, which lists them in the
        /// order to apply them: each comes after every Blueprint and enum it uses.
        #[arg(short, long)]
        out_dir: PathBuf,
        #[command(flatten)]
        project: ProjectArg,
        /// Print the result as JSON, with the fields of the plugin's report.
        #[arg(long)]
        json: bool,
    },
    /// Lift cooked Blueprints, structs and enums back to sources: each package's JSON, as
    /// CUE4Parse writes it, becomes a source in the project's Content beside where the asset
    /// would be, read against the types the plugin saved and against the other packages.
    Lift {
        /// The packages' JSON files.
        #[arg(required = true)]
        packages: Vec<PathBuf>,
        #[command(flatten)]
        project: ProjectArg,
    },
    /// Serve an editor as a language server, on stdin and stdout: what is wrong with the sources
    /// as they are typed, what a name is, and where the project's sources declare it.
    Lsp,
}

#[derive(clap::Args)]
struct ProjectArg {
    /// The project, or a folder in it. The current folder by default.
    #[arg(long = "project", value_name = "DIR", default_value = ".")]
    dir: PathBuf,
}

impl ProjectArg {
    fn find(&self) -> Result<project::Project> {
        project::Project::find(&self.dir)
    }
}

/// Prints a project's check, as text or JSON, and exits with 1 when it failed. As JSON, an error
/// that stopped it is one of its errors, so whatever reads it has one thing to read.
fn report(check: Result<project::Check>, json: bool) -> Result<()> {
    let check = match check {
        Ok(check) => check,
        Err(error) if json => project::Check {
            ok: false,
            sources: 0,
            errors: vec![project::Error::new("", format!("{error:#}"))],
            missing: Vec::new(),
        },
        Err(error) => return Err(error),
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&check)?);
    } else {
        print!("{}", check.text());
    }
    if !check.ok {
        std::process::exit(1);
    }
    Ok(())
}

/// Lifts the packages into the project's Content, saying what it wrote and what it could not
/// lift, and exits with 1 when any could not be lifted.
fn lift(packages: &[PathBuf], project: &project::Project) -> Result<()> {
    let db = project.type_db()?;
    let mut read = Vec::new();
    for path in packages {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        read.push(loom::lift::input::Package::parse(&text).with_context(|| format!("reading {}", path.display()))?);
    }
    let mut failed = std::collections::BTreeMap::new();
    for lifted in loom::lift::lift(&db, read)? {
        let relative = lifted.path.strip_prefix("/Game/").with_context(|| format!("{} is not under /Game", lifted.path))?;
        let file = project.root.join("Content").join(format!("{relative}.lm"));
        match lifted.source {
            Ok(source) => {
                std::fs::create_dir_all(file.parent().unwrap())?;
                std::fs::write(&file, source).with_context(|| format!("writing {}", file.display()))?;
                println!("wrote {}", file.display());
            }
            Err(err) => {
                // An error that is not a refusal is a fault in the lifter.
                let kind = match loom::lift::Refused::of(&err).map(|r| r.kind) {
                    Some(loom::lift::Refusal::Inexpressible) => "loom cannot express",
                    Some(loom::lift::Refusal::NotYet) => "not lifted yet",
                    Some(loom::lift::Refusal::Unexpected) => "unexpected input",
                    Some(loom::lift::Refusal::Missing) => "the project lacks it",
                    None => "a fault in the lifter",
                };
                *failed.entry(kind).or_insert(0) += 1;
                eprintln!("could not lift {}: {err:#}", lifted.path);
            }
        }
    }
    if !failed.is_empty() {
        let counts: Vec<String> = failed.iter().map(|(kind, n)| format!("{n} {kind}")).collect();
        eprintln!("{} not lifted: {}", failed.values().sum::<usize>(), counts.join(", "));
        std::process::exit(1);
    }
    Ok(())
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Docs { args } => match args.split_first() {
            None => print!("{}", docs::list()),
            Some((first, words)) if first == "search" => print!("{}", docs::search(&words.join(" "))),
            Some((page, [])) => print!("{}", docs::page(page)?),
            Some(_) => anyhow::bail!("loom docs takes one page, or `search` and some words"),
        },
        Command::Types { name, project } => print!("{}", project.find()?.types(&name)?),
        Command::Sources { project, json } => {
            let sources = project.find()?.sources()?;
            if json {
                let list: Vec<_> = sources.iter().map(|s| serde_json::json!({ "file": s.file, "package": s.package, "uses": s.uses() })).collect();
                println!("{}", serde_json::to_string_pretty(&list)?);
            } else {
                for source in &sources {
                    println!("{}  {}", source.file, source.package);
                }
            }
        }
        Command::Check { ops: Some(ops), .. } => {
            let text = std::fs::read_to_string(&ops).with_context(|| format!("reading {}", ops.display()))?;
            let doc = ops::Document::parse(&text).with_context(|| format!("checking {}", ops.display()))?;
            println!("{}: {} ops, ok", ops.display(), doc.ops.len());
        }
        Command::Check { project, json, ops: None } => report(project.find().and_then(|p| p.check()), json)?,
        Command::Build { out_dir, project, json } => {
            let built = project.find().and_then(|p| {
                let compiled = p.compile()?;
                compiled.write(&out_dir)?;
                Ok(compiled.check)
            });
            report(built, json)?
        }
        Command::Lift { packages, project } => lift(&packages, &project.find()?)?,
        Command::Lsp => lsp::run()?,
    }
    Ok(())
}

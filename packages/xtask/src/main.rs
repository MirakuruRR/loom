//! Tasks for working on Loom, run as `cargo xtask <task>`. Each works on an Unreal project, by
//! default the repo's own, `tests/project`, which holds no assets: the plugin and the examples are
//! linked into it. The engine is `--engine`, else `$UE_ROOT`, else the installed engine the
//! project's `EngineAssociation` names.

use std::collections::BTreeSet;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use serde_json::Value;

#[derive(Parser)]
#[command(about = "Tasks for working on Loom")]
struct Cli {
    #[command(subcommand)]
    task: Task,
}

#[derive(Subcommand)]
enum Task {
    /// Build the LoomEditor plugin against the engine and link it into the project, carrying the
    /// working tree's loom and loom-mcp.
    Plugin {
        #[command(flatten)]
        target: Target,
    },
    /// Build the LoomEditor plugin against the engine as it's released for that engine, carrying
    /// release builds of loom and loom-mcp, into out/package/<the engine's BuildId>/, and describe
    /// it in package.json there. Links it into no project; `examples` and `demos` take it with
    /// --plugin.
    Package {
        #[command(flatten)]
        target: Target,
    },
    /// Build the examples into the project with LoomBuild, as any project's sources are built, and
    /// run each example's test.json with LoomTest. Then check an enum, a struct and a Blueprint
    /// made the editor's way: loom refuses to overwrite them, and uses the enum and the struct.
    Examples {
        #[command(flatten)]
        target: Target,
        /// A plugin to link into the project and test, such as one `package` made, instead of the
        /// one `plugin` linked.
        #[arg(long, value_name = "DIR")]
        plugin: Option<PathBuf>,
    },
    /// Build the Whiskerwood demos into a Whiskerwood mod kit project with LoomBuild, each where the
    /// kit keeps a mod, and run each demo's test.json with LoomTest.
    Demos {
        /// The mod kit project's folder.
        #[arg(long, value_name = "DIR")]
        project: PathBuf,
        /// The engine's root folder.
        #[arg(long, value_name = "DIR")]
        engine: Option<PathBuf>,
        /// A plugin to link into the project and test, such as one `package` made, instead of the
        /// one `plugin` linked.
        #[arg(long, value_name = "DIR")]
        plugin: Option<PathBuf>,
    },
    /// Build each demo as a player installs it, <Name>.pak and <Name>.uplugin, into
    /// out/demo-mods/<Name>/: built and tested as `demos` does, labelled, and cooked in a
    /// Whiskerwood mod kit project.
    DemoMods {
        /// The mod kit project's folder.
        #[arg(long, value_name = "DIR")]
        project: PathBuf,
        /// The engine's root folder.
        #[arg(long, value_name = "DIR")]
        engine: Option<PathBuf>,
    },
    /// Remake packages/loom/tests/types/engine-<version>.json, the engine's own classes, structs
    /// and enums, from the test project, for the tests that run without Unreal.
    TestTypes {
        /// The engine's root folder.
        #[arg(long, value_name = "DIR")]
        engine: Option<PathBuf>,
    },
}

#[derive(clap::Args)]
struct Target {
    /// The Unreal project's folder. The repo's test project by default.
    #[arg(long, value_name = "DIR")]
    project: Option<PathBuf>,
    /// The engine's root folder.
    #[arg(long, value_name = "DIR")]
    engine: Option<PathBuf>,
}

#[cfg(target_os = "windows")]
mod platform {
    pub const NAME: &str = "Win64";
    pub const EXE: &str = ".exe";
    pub const EDITOR: &str = "Engine/Binaries/Win64/UnrealEditor-Cmd.exe";
    pub const UAT: &str = "Engine/Build/BatchFiles/RunUAT.bat";
    pub const BUILD: &str = "Engine/Build/BatchFiles/Build.bat";
    pub const PAK: &str = "Engine/Binaries/Win64/UnrealPak.exe";
    /// What a cook for this platform calls it in the names of its paks.
    pub const COOKED: &str = "Windows";
}
#[cfg(target_os = "macos")]
mod platform {
    pub const NAME: &str = "Mac";
    pub const EXE: &str = "";
    pub const EDITOR: &str = "Engine/Binaries/Mac/UnrealEditor.app/Contents/MacOS/UnrealEditor";
    pub const UAT: &str = "Engine/Build/BatchFiles/RunUAT.sh";
    pub const BUILD: &str = "Engine/Build/BatchFiles/Mac/Build.sh";
    pub const PAK: &str = "Engine/Binaries/Mac/UnrealPak";
    pub const COOKED: &str = "Mac";
}
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod platform {
    pub const NAME: &str = "Linux";
    pub const EXE: &str = "";
    pub const EDITOR: &str = "Engine/Binaries/Linux/UnrealEditor-Cmd";
    pub const UAT: &str = "Engine/Build/BatchFiles/RunUAT.sh";
    pub const BUILD: &str = "Engine/Build/BatchFiles/Linux/Build.sh";
    pub const PAK: &str = "Engine/Binaries/Linux/UnrealPak";
    pub const COOKED: &str = "Linux";
}

/// The programs the plugin carries.
const PROGRAMS: [&str; 2] = ["loom", "loom-mcp"];

fn repo() -> PathBuf {
    canonical(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).expect("the repo")
}

/// A path's canonical form, as a person types it. On Windows a canonical path starts `\\?\`,
/// which Unreal's tools can't read, so a drive path is given without it.
fn canonical(path: &Path) -> std::io::Result<PathBuf> {
    let path = path.canonicalize()?;
    let drive = path.to_str().and_then(|p| p.strip_prefix(r"\\?\")).filter(|p| p.as_bytes().get(1) == Some(&b':'));
    Ok(drive.map(PathBuf::from).unwrap_or(path))
}

fn test_project() -> PathBuf {
    repo().join("tests/project")
}

/// A project, and the engine that opens it.
struct Unreal {
    engine: PathBuf,
    uproject: PathBuf,
}

impl Unreal {
    fn new(project: Option<PathBuf>, engine: Option<PathBuf>) -> Result<Unreal> {
        let dir = project.unwrap_or_else(test_project);
        let dir = canonical(&dir).with_context(|| format!("reading {}", dir.display()))?;
        let uprojects: Vec<PathBuf> = std::fs::read_dir(&dir)?
            .map(|entry| entry.map(|e| e.path()))
            .filter(|path| path.as_ref().is_ok_and(|p| p.extension().is_some_and(|x| x == "uproject")))
            .collect::<std::io::Result<_>>()?;
        let [uproject] = uprojects.as_slice() else {
            bail!("expected one .uproject in {}, found {}", dir.display(), uprojects.len())
        };
        let engine = match engine.or_else(|| std::env::var_os("UE_ROOT").map(PathBuf::from)) {
            Some(engine) => engine,
            None => find_engine(uproject)?,
        };
        if !engine.join(platform::EDITOR).exists() {
            bail!("{} is not a built engine: it has no {}", engine.display(), platform::EDITOR);
        }
        Ok(Unreal { engine, uproject: uproject.clone() })
    }

    fn project(&self) -> &Path {
        self.uproject.parent().unwrap()
    }

    /// A name for the project in out/, unique to its folder.
    fn slug(&self) -> String {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::hash::DefaultHasher::new();
        self.project().hash(&mut hasher);
        let name = self.project().file_name().unwrap().to_string_lossy();
        format!("{name}-{:08x}", hasher.finish() as u32)
    }

    /// Runs one of the plugin's commandlets headless, and says what it reported of itself.
    /// Returns whether it succeeded; a run that wrote no report is an error.
    fn commandlet(&self, name: &str, out: &Path, args: &[String]) -> Result<bool> {
        if !self.project().join("Plugins/LoomEditor").exists() {
            bail!("LoomEditor is not in {}; run cargo xtask plugin", self.project().display());
        }
        let report = out.with_extension("report.json");
        let _ = std::fs::remove_file(&report);
        let mut args = args.to_vec();
        args.push(format!("-report={}", report.display()));
        let ok = self.commandlet_run(name, out, &args)?;
        if !report.exists() {
            bail!("{name} wrote no report; see {}", out.with_extension("log").display());
        }
        Ok(ok)
    }

    /// Runs a commandlet headless, logging to `out`.log, and says what it said of itself, its
    /// lines that start with its name. Runs on one project wait for each other, since two would
    /// write the same packages. Returns whether it succeeded.
    fn commandlet_run(&self, name: &str, out: &Path, args: &[String]) -> Result<bool> {
        let log = out.with_extension("log");
        std::fs::create_dir_all(out.parent().unwrap())?;
        let lock = File::create(repo().join(format!("out/.{}.lock", self.slug())))?;
        if lock.try_lock().is_err() {
            println!("another run is using {}; waiting", self.project().display());
            lock.lock()?;
        }
        println!("{name} on {} (log: {})", self.project().display(), log.display());
        let status = Command::new(self.engine.join(platform::EDITOR))
            .arg(&self.uproject)
            .arg(format!("-run={name}"))
            .args(args)
            .args(["-unattended", "-nosplash", "-nullrhi", "-nopause", "-stdout", "-NoLogTimes"])
            .stdout(File::create(&log)?)
            .stderr(File::create(&log)?)
            .status()
            .with_context(|| format!("running {}", self.engine.join(platform::EDITOR).display()))?;
        drop(lock);

        let text = std::fs::read_to_string(&log).unwrap_or_default();
        for line in text.lines() {
            let said = line.split_once(": ").map(|(_, rest)| rest);
            let said = said.and_then(|s| s.strip_prefix("Display: ").or_else(|| s.strip_prefix("Error: ")));
            if let Some(said) = said.filter(|s| s.starts_with(&format!("{name}:"))) {
                println!("{said}");
            }
        }
        if !status.success() {
            eprintln!("{name} exited with {status}; see {}", log.display());
        }
        Ok(status.success())
    }
}

/// The installed engine a project's EngineAssociation names: one the Epic launcher installed, or
/// one built from source and registered, whose own version is the one named.
fn find_engine(uproject: &Path) -> Result<PathBuf> {
    let text = std::fs::read_to_string(uproject)?;
    let association = serde_json::from_str::<Value>(&text)?["EngineAssociation"].as_str().unwrap_or("").to_string();
    if association.is_empty() {
        bail!("{} names no engine; give one with --engine or UE_ROOT", uproject.display());
    }
    let mut found = launcher_engines(&association);
    found.extend(registered_engines().into_iter().filter(|(key, path)| {
        *key == association || engine_version(path).as_deref() == Some(association.as_str())
    }).map(|(_, path)| path));
    match found.into_iter().find(|path| path.join(platform::EDITOR).exists()) {
        Some(engine) => {
            println!("engine: {}", engine.display());
            Ok(engine)
        }
        None => bail!(
            "no installed engine is {association}, which {} names; give one with --engine or UE_ROOT",
            uproject.display()
        ),
    }
}

/// Engines the Epic launcher installed, by the version they are.
fn launcher_engines(version: &str) -> Vec<PathBuf> {
    let dat = if cfg!(target_os = "windows") {
        std::env::var_os("ProgramData").map(|d| PathBuf::from(d).join("Epic/UnrealEngineLauncher/LauncherInstalled.dat"))
    } else if cfg!(target_os = "macos") {
        Some(PathBuf::from("/Users/Shared/Epic Games/UnrealEngineLauncher/LauncherInstalled.dat"))
    } else {
        None
    };
    let Some(list) = dat.and_then(|d| std::fs::read_to_string(d).ok()).and_then(|t| serde_json::from_str::<Value>(&t).ok()) else {
        return Vec::new();
    };
    list["InstallationList"].as_array().into_iter().flatten()
        .filter(|entry| entry["AppName"].as_str() == Some(&format!("UE_{version}")))
        .filter_map(|entry| entry["InstallLocation"].as_str().map(PathBuf::from))
        .collect()
}

/// Engines built from source and registered in the registry, with their keys: a project's
/// EngineAssociation names one, and its value is the folder the engine was built in.
#[cfg(target_os = "windows")]
fn registered_engines() -> Vec<(String, PathBuf)> {
    use windows_sys::Win32::Foundation::{ERROR_MORE_DATA, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{HKEY, HKEY_CURRENT_USER, KEY_READ, RegCloseKey, RegEnumValueW, RegOpenKeyExW};

    /// A string as the wide, null-terminated form a `…W` function takes.
    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// What a key holds: each value's name, which is an EngineAssociation, and its text.
    fn values(key: HKEY) -> Vec<(String, String)> {
        let mut found = Vec::new();
        let mut index = 0u32;
        loop {
            let mut name = [0u16; 256];
            let mut name_length = name.len() as u32;
            let mut kind = 0u32;
            let mut length = 0u32;
            // SAFETY: a null buffer asks how long the value is, and changes nothing.
            let asked = unsafe {
                RegEnumValueW(key, index, name.as_mut_ptr(), &mut name_length, std::ptr::null(), &mut kind, std::ptr::null_mut(), &mut length)
            };
            if asked != ERROR_SUCCESS && asked != ERROR_MORE_DATA {
                break;
            }
            // One value is read in two calls: the first asks how long its text is, the second reads
            // it. Each call wants its own buffer's length, so the name's is set again for the second.
            let mut data = vec![0u8; length as usize + 2];
            name_length = name.len() as u32;
            // SAFETY: the buffers are as long as the lengths the call above gave.
            let read = unsafe {
                RegEnumValueW(key, index, name.as_mut_ptr(), &mut name_length, std::ptr::null(), &mut kind, data.as_mut_ptr(), &mut length)
            };
            index += 1;
            if read != ERROR_SUCCESS {
                continue;
            }
            data.truncate(length as usize);
            let association = String::from_utf16_lossy(&name[..name_length as usize]);
            let text: Vec<u16> = data.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
            let path = String::from_utf16_lossy(&text);
            let path = path.trim_end_matches('\0').trim();
            if !association.is_empty() && !path.is_empty() {
                found.push((association, path.to_string()));
            }
        }
        found
    }

    let mut found = Vec::new();
    let mut builds: HKEY = std::ptr::null_mut();
    // SAFETY: the subkey is a live wide string, and the key is closed once it has been read.
    let opened = unsafe {
        RegOpenKeyExW(HKEY_CURRENT_USER, wide(r"Software\Epic Games\Unreal Engine\Builds").as_ptr(), 0, KEY_READ, &mut builds)
    };
    if opened == ERROR_SUCCESS {
        found.extend(values(builds).into_iter().map(|(association, path)| (association, PathBuf::from(path))));
        // SAFETY: the key was opened just above, and is closed once.
        unsafe { RegCloseKey(builds) };
    }
    found
}

/// Engines built from source and registered in Install.ini, with their keys.
#[cfg(not(target_os = "windows"))]
fn registered_engines() -> Vec<(String, PathBuf)> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else { return Vec::new() };
    let ini = if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Epic/UnrealEngine/Install.ini")
    } else {
        home.join(".config/Epic/UnrealEngine/Install.ini")
    };
    let Ok(text) = std::fs::read_to_string(ini) else { return Vec::new() };
    text.lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, path)| (key.trim().to_string(), PathBuf::from(path.trim())))
        .collect()
}

/// An engine's version, as `5.6`.
fn engine_version(engine: &Path) -> Option<String> {
    let text = std::fs::read_to_string(engine.join("Engine/Build/Build.version")).ok()?;
    let version: Value = serde_json::from_str(&text).ok()?;
    Some(format!("{}.{}", version["MajorVersion"].as_u64()?, version["MinorVersion"].as_u64()?))
}

/// An engine's full version, as `5.6.1`.
fn engine_full_version(engine: &Path) -> Option<String> {
    let text = std::fs::read_to_string(engine.join("Engine/Build/Build.version")).ok()?;
    let version: Value = serde_json::from_str(&text).ok()?;
    Some(format!("{}.{}.{}", version["MajorVersion"].as_u64()?, version["MinorVersion"].as_u64()?, version["PatchVersion"].as_u64()?))
}

/// Links `link` to `target`, replacing a link already there but nothing else.
fn link(target: &Path, link: &Path) -> Result<()> {
    if link.is_symlink() {
        if std::fs::read_link(link)? == target {
            return Ok(());
        }
        remove_link(link)?;
    } else if link.exists() {
        bail!("{} exists and is not a link; remove it by hand", link.display());
    }
    if let Some(parent) = link.parent() {
        std::fs::create_dir_all(parent)?;
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link)?;
    #[cfg(windows)]
    {
        let made = if target.is_dir() {
            std::os::windows::fs::symlink_dir(target, link)
        } else {
            std::os::windows::fs::symlink_file(target, link)
        };
        made.context("making a link, which Windows allows with Developer Mode on")?;
    }
    Ok(())
}

fn remove_link(link: &Path) -> Result<()> {
    // A link to a folder is removed as a folder on Windows, and as a file elsewhere.
    std::fs::remove_file(link).or_else(|_| std::fs::remove_dir(link))?;
    Ok(())
}

/// Builds the programs the plugin carries, as the working tree has them, as a release build or
/// not.
fn build_programs(release: bool) -> Result<()> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo)
        .current_dir(repo())
        .args(["build", "--quiet", "--package", "loom-cli", "--package", "loom-mcp"])
        .args(if release { &["--release", "--locked"][..] } else { &[] })
        .status()?;
    if !status.success() {
        bail!("building loom and loom-mcp failed");
    }
    Ok(())
}

fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| repo().join("target"))
}

fn plugin(unreal: &Unreal) -> Result<()> {
    build_programs(false)?;
    // One build per project, so a build for one project never deletes the plugin another project,
    // or its open editor, is using.
    let out = repo().join("out").join(unreal.slug()).join("plugin/LoomEditor");
    build_plugin(unreal, &out)?;
    let binaries = out.join("Binaries/ThirdParty/Loom").join(platform::NAME);
    for program in PROGRAMS {
        let file = format!("{program}{}", platform::EXE);
        link(&target_dir().join("debug").join(&file), &binaries.join(&file))?;
    }
    link(&out, &unreal.project().join("Plugins/LoomEditor"))?;
    println!("built and linked: {}", unreal.project().join("Plugins/LoomEditor").display());
    Ok(())
}

fn package(unreal: &Unreal) -> Result<()> {
    build_programs(true)?;
    let build_id = read_build_id(&unreal.engine.join("Engine/Binaries").join(platform::NAME))?;
    let dir = repo().join("out/package").join(&build_id);
    let out = dir.join("LoomEditor");
    build_plugin(unreal, &out)?;
    let binaries = out.join("Binaries/ThirdParty/Loom").join(platform::NAME);
    std::fs::create_dir_all(&binaries)?;
    for program in PROGRAMS {
        let file = format!("{program}{}", platform::EXE);
        std::fs::copy(target_dir().join("release").join(&file), binaries.join(&file))?;
    }
    // The plugin's modules load only in an engine with the BuildId they were built with.
    let built = read_build_id(&out.join("Binaries").join(platform::NAME))?;
    if built != build_id {
        bail!("the plugin was built with BuildId {built}, but the engine's is {build_id}");
    }
    let log = std::fs::read_to_string(out.with_extension("log"))?;
    let toolchain = log.lines().map(str::trim).find(|l| l.starts_with("Using ") && l.contains("toolchain"))
        .context("the build's log names no toolchain")?;
    let commit = Command::new("git").current_dir(repo()).args(["rev-parse", "HEAD"]).output()?;
    let description = serde_json::json!({
        "loom": env!("CARGO_PKG_VERSION"),
        "commit": String::from_utf8(commit.stdout)?.trim(),
        "platform": platform::NAME,
        "build_id": build_id,
        "engine_version": engine_full_version(&unreal.engine).context("the engine has no Build.version")?,
        "toolchain": toolchain,
    });
    std::fs::write(dir.join("package.json"), serde_json::to_string_pretty(&description)? + "\n")?;
    println!("packaged for BuildId {build_id}: {}", dir.display());
    Ok(())
}

/// The BuildId of the modules in a Binaries/<platform> folder, which is the engine's when the
/// folder is the engine's own.
fn read_build_id(binaries: &Path) -> Result<String> {
    let path = binaries.join("UnrealEditor.modules");
    let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let modules: Value = serde_json::from_str(&text)?;
    Ok(modules["BuildId"].as_str().with_context(|| format!("{} has no BuildId", path.display()))?.to_string())
}

/// Builds the plugin against the project's engine into `out`, as BuildPlugin packages it, with
/// no programs.
fn build_plugin(unreal: &Unreal, out: &Path) -> Result<()> {
    if out.exists() {
        std::fs::remove_dir_all(out)?;
    }
    std::fs::create_dir_all(out.parent().unwrap())?;
    let log = out.with_extension("log");
    println!("building LoomEditor with {} (log: {})", unreal.engine.display(), log.display());
    let status = Command::new(unreal.engine.join(platform::UAT))
        .arg("BuildPlugin")
        .arg(format!("-Plugin={}", repo().join("plugin/LoomEditor/LoomEditor.uplugin").display()))
        .arg(format!("-Package={}", out.display()))
        .arg(format!("-TargetPlatforms={}", platform::NAME))
        .arg("-Rocket")
        .stdout(File::create(&log)?)
        .stderr(File::create(&log)?)
        .status()?;
    if !status.success() {
        let text = std::fs::read_to_string(&log).unwrap_or_default();
        for line in text.lines().filter(|l| l.contains("error") || l.contains("FAILED")).take(30) {
            eprintln!("{line}");
        }
        bail!("building the plugin failed; see {}", log.display());
    }
    // The packaged copy is the one the editor loads.
    for scratch in ["HostProject", "Intermediate"] {
        let _ = std::fs::remove_dir_all(out.join(scratch));
    }
    // One version names every package, the workspace's. The build names itself the same, so the
    // editor and the compiler never report two versions of one Loom.
    let manifest = out.join("LoomEditor.uplugin");
    let mut plugin: Value = serde_json::from_str(&std::fs::read_to_string(&manifest)?)?;
    plugin["VersionName"] = Value::String(env!("CARGO_PKG_VERSION").to_string());
    std::fs::write(&manifest, serde_json::to_string_pretty(&plugin)? + "\n")?;
    Ok(())
}

/// Links a plugin into the project to be tested, in place of the one there.
fn use_plugin(unreal: &Unreal, plugin: &Path) -> Result<()> {
    let plugin = canonical(plugin).with_context(|| format!("reading {}", plugin.display()))?;
    if !plugin.join("LoomEditor.uplugin").exists() {
        bail!("{} is not the LoomEditor plugin: it has no LoomEditor.uplugin", plugin.display());
    }
    link(&plugin, &unreal.project().join("Plugins/LoomEditor"))?;
    println!("testing the plugin in {}", plugin.display());
    Ok(())
}

fn examples(unreal: &Unreal, plugin: Option<&Path>) -> Result<()> {
    if let Some(plugin) = plugin {
        use_plugin(unreal, plugin)?;
    }
    // Each example's files are linked into out/, which the project links as Content/Loom/Examples,
    // so each source sits beside its Blueprint and the Blueprints are made in out/.
    let out = repo().join("out").join(unreal.slug());
    // The checks after the examples start over each run, so none of their sources is left for the
    // examples' build.
    let foreign = out.join("foreign");
    let reorder = out.join("reorder");
    for (dir, content) in [(&foreign, "Content/Loom/Foreign"), (&reorder, "Content/Loom/Reorder")] {
        if dir.exists() {
            std::fs::remove_dir_all(dir)?;
        }
        std::fs::create_dir_all(dir)?;
        link(dir, &unreal.project().join(content))?;
    }
    let content = out.join("examples");
    let mut tests = Vec::new();
    for example in sorted_dir(&repo().join("examples"))? {
        link_files(&example, &content.join(example.file_name().unwrap()))?;
        if example.join("test.json").exists() {
            tests.push(example.join("test.json"));
        }
    }
    link(&content, &unreal.project().join("Content/Loom/Examples"))?;
    // Every example is built again, so a change to how the plugin applies ops is tested even
    // before the version that records it changes.
    build_and_test(unreal, &out.join("examples-run"), true, plugin.is_none(), &tests)?;
    check_foreign(unreal, &foreign, &out.join("foreign-run"))?;
    check_reorder(unreal, &reorder, &out.join("reorder-run"))
}

/// Checks that an enum's values reordered while a Blueprint holding one is loaded keep what it
/// holds: in one editor, `content` is built, the enum's source reordered and built again, and the
/// Blueprint saved as it was in memory; then its test reads the value back.
fn check_reorder(unreal: &Unreal, content: &Path, out: &Path) -> Result<()> {
    let fixtures = repo().join("packages/xtask/src/reorder");
    for file in ["E_Order.lm", "BP_Order.lm"] {
        std::fs::copy(fixtures.join(file), content.join(file))?;
    }
    let script = out.join("reorder.py");
    std::fs::create_dir_all(out)?;
    let head = format!("CONTENT = {:?}\nREORDERED = {:?}\n", content.display().to_string(), fixtures.join("E_Order.reordered.lm").display().to_string());
    std::fs::write(&script, head + include_str!("reorder/reorder.py"))?;
    let args = [format!("-script={}", script.display()), "-EnablePlugins=PythonScriptPlugin".to_string()];
    let ran = unreal.commandlet_run("pythonscript", &out.join("script"), &args)?;
    if !ran || !std::fs::read_to_string(out.join("script.log"))?.contains("Python script executed successfully") {
        bail!("reordering the enum failed; see {}", out.join("script.log").display());
    }
    if !unreal.commandlet("LoomTest", &out.join("test"), &[format!("-test={}", fixtures.join("test.json").display())])? {
        bail!("a value held while its enum was reordered changed; see {}", out.join("test.log").display());
    }
    println!("a value held while its enum was reordered kept its meaning");
    Ok(())
}

/// Checks what loom does with an enum, a struct and a Blueprint it didn't make, made in `content`
/// the editor's way: a source aimed at one is refused and leaves it as it was, and a source uses
/// the enum and struct by the names the editor shows.
fn check_foreign(unreal: &Unreal, content: &Path, out: &Path) -> Result<()> {
    let fixtures = repo().join("packages/xtask/src/foreign");
    let args = [format!("-script={}", fixtures.join("make.py").display()), "-EnablePlugins=PythonScriptPlugin".to_string()];
    if !unreal.commandlet_run("pythonscript", &out.join("make"), &args)? {
        bail!("making the editor's enum, struct and Blueprint failed");
    }
    let assets = ["E_Hand", "S_Hand", "BP_Hand"];
    let read = || -> Result<Vec<Vec<u8>>> { Ok(assets.iter().map(|a| std::fs::read(content.join(format!("{a}.uasset")))).collect::<std::io::Result<_>>()?) };
    let before = read()?;

    for asset in assets {
        std::fs::copy(fixtures.join("refused").join(format!("{asset}.lm")), content.join(format!("{asset}.lm")))?;
    }
    let run = out.join("refused");
    unreal.commandlet("LoomBuild", &run, &[])?;
    for asset in assets {
        std::fs::remove_file(content.join(format!("{asset}.lm")))?;
    }
    let report: Value = serde_json::from_str(&std::fs::read_to_string(run.with_extension("report.json"))?)?;
    for asset in assets {
        let file = format!("Content/Loom/Foreign/{asset}.lm");
        let refused = report["errors"].as_array().into_iter().flatten()
            .any(|e| e["file"] == file.as_str() && e["message"].as_str().is_some_and(|m| m.contains("was not made by loom")));
        let failed = report["blueprints"].as_array().into_iter().flatten().any(|b| b["source"] == file.as_str() && b["status"] == "failed");
        if !refused || !failed {
            bail!("{file} was not refused as aimed at an asset loom didn't make; see {}", run.with_extension("report.json").display());
        }
    }
    if read()? != before {
        bail!("a refused build changed an asset loom didn't make");
    }
    println!("sources aimed at an enum, a struct and a Blueprint loom didn't make were refused");

    std::fs::copy(fixtures.join("used/BP_LoomForeign.lm"), content.join("BP_LoomForeign.lm"))?;
    build_and_test(unreal, &out.join("used"), false, false, &[fixtures.join("used/test.json")])
}

fn demos(unreal: &Unreal, plugin: Option<&Path>) -> Result<()> {
    if let Some(plugin) = plugin {
        use_plugin(unreal, plugin)?;
    }
    // Each demo's mod folder, Mods/<Name>, is linked file by file into out/, and that into the
    // kit project as Content/Mods/<Name>, where the kit keeps a mod. The Blueprints are made in
    // out/, not in the repo.
    let out = repo().join("out").join(unreal.slug());
    let mut tests = Vec::new();
    for demo in sorted_dir(&repo().join("demos"))?.into_iter().filter(|d| d.join("Mods").is_dir()) {
        for module in sorted_dir(&demo.join("Mods"))? {
            let name = module.file_name().unwrap();
            let content = out.join("demos").join(name);
            link_files(&module, &content)?;
            link(&content, &unreal.project().join("Content/Mods").join(name))?;
        }
        if demo.join("test.json").exists() {
            tests.push(demo.join("test.json"));
        }
    }
    // Not forced: the kit project holds other mods, whose Blueprints a forced build would rewrite.
    build_and_test(unreal, &out.join("demos-run"), false, plugin.is_none(), &tests)
}

/// The first pak chunk the demos' labels use, clear of the chunks other mods commonly take.
const FIRST_DEMO_CHUNK: i64 = 90;

fn demo_mods(unreal: &Unreal) -> Result<()> {
    demos(unreal, None)?;
    let out = repo().join("out").join(unreal.slug());
    let names: Vec<String> = sorted_dir(&out.join("demos"))?
        .iter()
        .map(|dir| dir.file_name().unwrap().to_string_lossy().to_string())
        .collect();

    // Each demo's folder gets its own chunk, so the cook makes a pak of it alone.
    let labels: Vec<(String, i64)> = names.iter().cloned().zip(FIRST_DEMO_CHUNK..).collect();
    let script = out.join("labels.py");
    std::fs::write(&script, format!("LABELS = {}\n{}", serde_json::to_string(&labels)?, include_str!("labels.py")))?;
    let args = [format!("-script={}", script.display()), "-EnablePlugins=PythonScriptPlugin".to_string()];
    if !unreal.commandlet_run("pythonscript", &out.join("labels"), &args)? {
        bail!("labelling the demos failed");
    }

    // Staging needs the game's own target built once, though the demos need nothing compiled.
    let game = unreal.uproject.file_stem().unwrap().to_string_lossy().to_string();
    let receipt = unreal.project().join(format!("Binaries/{0}/{game}-{0}-Shipping.target", platform::NAME));
    if !receipt.exists() {
        run_logged(
            Command::new(unreal.engine.join(platform::BUILD))
                .args([&game, platform::NAME, "Shipping"])
                .arg(format!("-Project={}", unreal.uproject.display()))
                .arg("-WaitMutex"),
            &out.join("game-target.log"),
            "building the game's target",
        )?;
    }
    let staged = out.join("cook/Packaged");
    run_logged(
        Command::new(unreal.engine.join(platform::UAT))
            .arg("BuildCookRun")
            .arg(format!("-project={}", unreal.uproject.display()))
            .arg(format!("-platform={}", platform::NAME))
            .args(["-clientconfig=Shipping", "-cook", "-stage", "-pak", "-skipbuild", "-nocompileeditor", "-noP4"])
            .args(["-unattended", "-utf8output"])
            .arg(format!("-stagingdirectory={}", staged.display())),
        &out.join("cook.log"),
        "cooking",
    )?;

    // Each pak must hold its demo's folder and nothing else, installed as <Name>.pak beside its
    // .uplugin, as the game loads a mod.
    let paks = staged.join(platform::COOKED).join(&game).join("Content/Paks");
    for (name, chunk) in &labels {
        let pak = paks.join(format!("pakchunk{chunk}-{}.pak", platform::COOKED));
        let listing = Command::new(unreal.engine.join(platform::PAK)).arg(&pak).arg("-List").output()?;
        let listing = String::from_utf8_lossy(&listing.stdout);
        let mount = format!("/{game}/Content/Mods/{name}/");
        if !listing.lines().any(|l| l.contains("mount point") && l.contains(&mount)) {
            bail!("{} doesn't hold only {name}'s folder:\n{listing}", pak.display());
        }
        let dest = repo().join("out/demo-mods").join(name);
        std::fs::create_dir_all(&dest)?;
        std::fs::copy(&pak, dest.join(format!("{name}.pak")))?;
        std::fs::copy(out.join("demos").join(name).join(format!("{name}.uplugin")), dest.join(format!("{name}.uplugin")))?;
        println!("{name}: {}", dest.display());
    }
    Ok(())
}

/// Runs a program to a log, and on failure says what it said of errors.
fn run_logged(command: &mut Command, log: &Path, what: &str) -> Result<()> {
    println!("{what} (log: {})", log.display());
    let status = command.stdout(File::create(log)?).stderr(File::create(log)?).status()?;
    if !status.success() {
        let text = std::fs::read_to_string(log).unwrap_or_default();
        for line in text.lines().filter(|l| l.contains("rror")).take(20) {
            eprintln!("{line}");
        }
        bail!("{what} failed ({status}); see {}", log.display());
    }
    Ok(())
}

/// Links each file of `from` into `to`, and removes links there to files since removed.
fn link_files(from: &Path, to: &Path) -> Result<()> {
    for file in sorted_dir(from)? {
        link(&file, &to.join(file.file_name().unwrap()))?;
    }
    for file in sorted_dir(to)? {
        if file.is_symlink() && !file.exists() {
            remove_link(&file)?;
        }
    }
    Ok(())
}

/// Builds the project's sources with LoomBuild, and runs each test with LoomTest. The programs the
/// plugin `plugin` linked carries are the working tree's, built first when `programs` is set.
fn build_and_test(unreal: &Unreal, out: &Path, force: bool, programs: bool, tests: &[PathBuf]) -> Result<()> {
    if programs {
        build_programs(false)?;
    }
    let args = if force { vec!["-force".to_string()] } else { Vec::new() };
    if !unreal.commandlet("LoomBuild", &out.join("build"), &args)? {
        bail!("the build failed");
    }
    let mut failed = Vec::new();
    for test in tests {
        let name = test.parent().unwrap().file_name().unwrap().to_string_lossy().to_string();
        if !unreal.commandlet("LoomTest", &out.join("test").join(&name), &[format!("-test={}", test.display())])? {
            failed.push(name);
        }
    }
    if !failed.is_empty() {
        bail!("these failed their tests: {}", failed.join(", "));
    }
    println!("built, and {} tests passed", tests.len());
    Ok(())
}

fn sorted_dir(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<std::io::Result<_>>()?;
    paths.sort();
    Ok(paths)
}

/// The modules whose Build.cs is under the engine: the engine's own, and its plugins'.
fn engine_modules(engine: &Path) -> Result<BTreeSet<String>> {
    let mut modules = BTreeSet::new();
    let mut dirs = vec![engine.join("Engine")];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                dirs.push(path);
            } else if let Some(module) = path.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_suffix(".Build.cs")) {
                modules.insert(module.to_string());
            }
        }
    }
    Ok(modules)
}

/// The module a path such as `/Script/Engine.Actor` is in.
fn module_of(path: &str) -> Option<&str> {
    path.strip_prefix("/Script/")?.split('.').next()
}

/// The files that decide what `test-types` dumps: the dumper, and the test project, which names
/// the engine and the plugins it has on. The fixture keeps each one's hash, and a test says when
/// one has changed since.
const TEST_TYPES_INPUTS: &[&str] = &[
    "plugin/LoomEditor/Source/LoomEditor/Private/LoomTypes.cpp",
    "plugin/LoomEditor/Source/LoomEditor/Private/LoomTypesCommandlet.cpp",
    "plugin/LoomEditor/Source/LoomEditor/Public/LoomTypes.h",
    "plugin/LoomEditor/Source/LoomEditor/Public/LoomTypesCommandlet.h",
    "tests/project/LoomTests.uproject",
];

fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn test_types(unreal: &Unreal) -> Result<()> {
    let out = repo().join("out").join(unreal.slug()).join("types");
    if !unreal.commandlet("LoomTypes", &out, &[])? {
        bail!("dumping the types failed");
    }
    let dump: Value = serde_json::from_str(&std::fs::read_to_string(out.with_extension("report.json"))?)?;
    let modules = engine_modules(&unreal.engine)?;
    let is_engine = |path: &str| module_of(path).is_some_and(|m| modules.contains(m));

    // Only the types under /Script/<an engine module>, such as nothing of the plugin's own, and
    // none of the project's settings.
    let mut kept = serde_json::Map::new();
    for kind in ["classes", "structs", "enums"] {
        let Value::Object(entries) = dump[kind].clone() else { bail!("the dump's {kind} is not an object") };
        kept.insert(kind.to_string(), Value::Object(entries.into_iter().filter(|(path, _)| is_engine(path)).collect()));
    }
    let Some(version) = engine_full_version(&unreal.engine) else { bail!("{} says no version", unreal.engine.display()) };
    let mut files = serde_json::Map::new();
    for file in TEST_TYPES_INPUTS {
        let bytes = std::fs::read(repo().join(file)).with_context(|| format!("reading {file}"))?;
        files.insert(file.to_string(), Value::String(sha256(&bytes)));
    }
    kept.insert("inputs".to_string(), serde_json::json!({ "engine": version, "files": files }));
    let kept = Value::Object(kept);

    // Anything left that names a type outside the engine would not resolve.
    let mut leaks = BTreeSet::new();
    find_leaks(&kept, &is_engine, &mut leaks);
    if !leaks.is_empty() {
        let some: Vec<_> = leaks.into_iter().take(5).collect();
        bail!("the engine's types refer to types outside it, such as: {}", some.join(", "));
    }

    let minor = version.rsplit_once('.').map_or(version.as_str(), |(minor, _)| minor);
    let path = repo().join(format!("packages/loom/tests/types/engine-{minor}.json"));
    std::fs::write(&path, serde_json::to_string_pretty(&sorted(kept))? + "\n")?;
    let count = |kind: &str| path_count(&path, kind);
    println!(
        "{}: {} classes, {} structs, {} enums, from {}",
        path.display(), count("classes")?, count("structs")?, count("enums")?, unreal.engine.display()
    );
    Ok(())
}

fn find_leaks(value: &Value, is_engine: &dyn Fn(&str) -> bool, leaks: &mut BTreeSet<String>) {
    match value {
        Value::String(s) if s.starts_with("/Game/") || (s.starts_with("/Script/") && !is_engine(s)) => {
            leaks.insert(s.clone());
        }
        Value::Array(items) => items.iter().for_each(|v| find_leaks(v, is_engine, leaks)),
        Value::Object(fields) => fields.values().for_each(|v| find_leaks(v, is_engine, leaks)),
        _ => {}
    }
}

/// A value with every object's keys in order, so a new dump shows as a diff of what changed.
fn sorted(value: Value) -> Value {
    match value {
        Value::Object(fields) => {
            let mut fields: Vec<_> = fields.into_iter().collect();
            fields.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(fields.into_iter().map(|(k, v)| (k, sorted(v))).collect())
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sorted).collect()),
        other => other,
    }
}

fn path_count(path: &Path, kind: &str) -> Result<usize> {
    let value: Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    Ok(value[kind].as_object().map_or(0, |o| o.len()))
}

fn main() -> Result<()> {
    match Cli::parse().task {
        Task::Plugin { target } => plugin(&Unreal::new(target.project, target.engine)?),
        Task::Package { target } => package(&Unreal::new(target.project, target.engine)?),
        Task::Examples { target, plugin } => examples(&Unreal::new(target.project, target.engine)?, plugin.as_deref()),
        Task::Demos { project, engine, plugin } => demos(&Unreal::new(Some(project), engine)?, plugin.as_deref()),
        Task::DemoMods { project, engine } => demo_mods(&Unreal::new(Some(project), engine)?),
        Task::TestTypes { engine } => test_types(&Unreal::new(None, engine)?),
    }
}

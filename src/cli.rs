use crate::check;
use crate::config::{CONFIG_FILE, Config};
use crate::fsutil;
use crate::lockfile::{LOCK_FILE, LockFile};
use crate::runtime::{DEFAULT_CHANNEL, RuntimeRegistry};
use crate::targets;
use crate::{LovelyError, Result};
use std::env;
use std::path::Path;

pub fn run() -> Result<()> {
    let mut args = env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        print_help();
        return Ok(());
    }

    let command = args.remove(0);
    let root = env::current_dir().map_err(LovelyError::plain_io)?;
    match command.as_str() {
        "init" => init(&root),
        "lock" => lock(&root),
        "doctor" => doctor(&root, args.first().map(String::as_str)),
        "check" => check_command(&root, &args),
        "build" => build(&root, args.first().map(String::as_str).unwrap_or("all")),
        "runtime" => runtime_command(&args),
        "publish" => publish(&root, &args),
        "ci" => ci(&root, args.first().map(String::as_str).unwrap_or("github")),
        "help" => {
            print_help();
            Ok(())
        }
        other => Err(LovelyError::Command(format!(
            "unknown command {other:?}; run lovely --help"
        ))),
    }
}

fn runtime_command(args: &[String]) -> Result<()> {
    let Some(command) = args.first().map(String::as_str) else {
        print_runtime_help();
        return Ok(());
    };
    match command {
        "fetch" => runtime_fetch(&args[1..]),
        "doctor" => runtime_doctor(args.get(1).map(String::as_str)),
        "list" => runtime_list(),
        "cache-dir" => {
            println!("{}", RuntimeRegistry::new().root().display());
            Ok(())
        }
        "help" | "--help" | "-h" => {
            print_runtime_help();
            Ok(())
        }
        other => Err(LovelyError::Command(format!(
            "unknown runtime command {other:?}; run lovely runtime help"
        ))),
    }
}

fn runtime_fetch(args: &[String]) -> Result<()> {
    if args.len() < 2 {
        return Err(LovelyError::Command(
            "usage: lovely runtime fetch <target> <local-path> [--channel love-11-plus] [--sha256 <hex>]".to_string(),
        ));
    }

    let target = &args[0];
    let source = Path::new(&args[1]);
    let mut channel = DEFAULT_CHANNEL.to_string();
    let mut expected_sha256 = None::<String>;
    let mut index = 2;
    while index < args.len() {
        match args[index].as_str() {
            "--channel" => {
                let Some(value) = args.get(index + 1) else {
                    return Err(LovelyError::Command(
                        "--channel requires a value".to_string(),
                    ));
                };
                channel = value.clone();
                index += 2;
            }
            "--sha256" => {
                let Some(value) = args.get(index + 1) else {
                    return Err(LovelyError::Command(
                        "--sha256 requires a value".to_string(),
                    ));
                };
                expected_sha256 = Some(value.clone());
                index += 2;
            }
            other => {
                return Err(LovelyError::Command(format!(
                    "unknown runtime fetch option {other:?}"
                )));
            }
        }
    }

    let registry = RuntimeRegistry::new();
    let manifest = registry.install_local(target, &channel, source, expected_sha256.as_deref())?;
    println!(
        "Installed {} runtime for channel {}",
        manifest.target, manifest.channel
    );
    println!("  sha256 {}", manifest.sha256);
    println!(
        "  path {}",
        registry
            .root()
            .join(&manifest.channel)
            .join(&manifest.target)
            .join(&manifest.path)
            .display()
    );
    Ok(())
}

fn runtime_doctor(target: Option<&str>) -> Result<()> {
    let registry = RuntimeRegistry::new();
    let targets = match target {
        Some("all") | None => vec!["web", "windows", "macos", "linux"],
        Some(target) => vec![target],
    };

    let mut missing = false;
    for target in targets {
        crate::runtime::validate_target(target)?;
        match registry.find(target, DEFAULT_CHANNEL)? {
            Some(runtime) if runtime.path.exists() => {
                println!(
                    "ok[{target}] {} {} {}",
                    runtime.manifest.channel,
                    runtime.manifest.sha256,
                    runtime.path.display()
                );
            }
            Some(runtime) => {
                missing = true;
                println!(
                    "missing[{target}] manifest exists but artifact is absent: {}",
                    runtime.path.display()
                );
            }
            None => {
                missing = true;
                println!("missing[{target}] no {DEFAULT_CHANNEL} runtime installed");
            }
        }
    }

    if missing {
        return Err(LovelyError::Command(
            "one or more runtimes are missing".to_string(),
        ));
    }
    Ok(())
}

fn runtime_list() -> Result<()> {
    let registry = RuntimeRegistry::new();
    let runtimes = registry.list()?;
    if runtimes.is_empty() {
        println!(
            "No Lovely runtimes installed in {}",
            registry.root().display()
        );
        return Ok(());
    }

    for runtime in runtimes {
        println!(
            "{} {} {:?} {} {}",
            runtime.manifest.channel,
            runtime.manifest.target,
            runtime.manifest.kind,
            runtime.manifest.sha256,
            runtime.path.display()
        );
    }
    Ok(())
}

fn init(root: &Path) -> Result<()> {
    let config_path = root.join(CONFIG_FILE);
    if config_path.exists() {
        return Err(LovelyError::Command(format!(
            "{} already exists",
            config_path.display()
        )));
    }

    let config = Config::default_for_dir(root);
    fsutil::write_string(&config_path, &config.to_toml())?;
    ensure_lock(root)?;
    println!("Created {}", config_path.display());
    println!("Created {}", root.join(LOCK_FILE).display());
    Ok(())
}

fn lock(root: &Path) -> Result<()> {
    let path = root.join(LOCK_FILE);
    let lock = if path.exists() {
        LockFile::load_from(&path)?
    } else {
        LockFile::preview_default()
    };
    fsutil::write_string(&path, &lock.to_text())?;
    println!("Wrote {}", path.display());
    if lock.has_unresolved_checksums() {
        println!(
            "Note: runtime checksums are unresolved until upstream runtime artifacts are installed or resolved."
        );
    }
    Ok(())
}

fn doctor(root: &Path, target: Option<&str>) -> Result<()> {
    let config = load_config(root)?;
    let lock = load_lock(root)?;
    let target = target.unwrap_or("all");
    let mut report = check::DiagnosticReport::default();

    for name in targets::expand_targets(target) {
        let adapter = targets::adapter_for(name).ok_or_else(|| unknown_target(name))?;
        report.extend(adapter.doctor(root, &config, &lock)?);
    }

    print!("{}", report.render());
    if report.has_errors() {
        return Err(LovelyError::Command(
            "doctor found blocking issues".to_string(),
        ));
    }
    Ok(())
}

fn check_command(root: &Path, args: &[String]) -> Result<()> {
    let config = load_config(root)?;
    let targets = if args.is_empty() {
        Vec::new()
    } else {
        args.to_vec()
    };
    let report = check::check_project(root, &config, &targets)?;
    print!("{}", report.render());
    if report.has_errors() {
        return Err(LovelyError::Command(
            "compatibility check failed".to_string(),
        ));
    }
    Ok(())
}

fn build(root: &Path, target: &str) -> Result<()> {
    let config = load_config(root)?;
    let lock = load_lock(root)?;
    let expanded = targets::expand_targets(target);
    if expanded.is_empty() {
        return Err(unknown_target(target));
    }

    for name in expanded {
        let adapter = targets::adapter_for(name).ok_or_else(|| unknown_target(name))?;
        let output = adapter.build(root, &config, &lock)?;
        println!("Built {}:", output.target);
        for artifact in output.artifacts {
            println!("  {}", artifact.display());
        }
    }
    Ok(())
}

fn publish(root: &Path, args: &[String]) -> Result<()> {
    let Some(provider) = args.first().map(String::as_str) else {
        return Err(LovelyError::Command(
            "publish requires a provider; currently supported: itch".to_string(),
        ));
    };
    if provider != "itch" {
        return Err(LovelyError::Command(format!(
            "unsupported publish provider {provider:?}; currently supported: itch"
        )));
    }
    let config = load_config(root)?;
    let Some(project) = config.itch.project.as_deref() else {
        return Err(LovelyError::Command(
            "itch.project must be configured before publishing".to_string(),
        ));
    };

    let artifact = root
        .join(&config.paths.output)
        .join(format!("{}-web.zip", config.game.id));
    if !artifact.is_file() {
        return Err(LovelyError::Command(format!(
            "{} does not exist; run lovely build web first",
            artifact.display()
        )));
    }

    println!(
        "Would publish {} to itch.io project {project}.",
        artifact.display()
    );
    println!("Install Butler and run:");
    println!(
        "  butler push {} {}:{}",
        artifact.display(),
        project,
        config.itch.prerelease_channel
    );
    Ok(())
}

fn ci(root: &Path, provider: &str) -> Result<()> {
    if provider != "github" {
        return Err(LovelyError::Command(format!(
            "unsupported CI provider {provider:?}; currently supported: github"
        )));
    }

    let path = root.join(".github/workflows/lovely.yml");
    fsutil::write_string(&path, github_actions())?;
    println!("Wrote {}", path.display());
    Ok(())
}

fn ensure_lock(root: &Path) -> Result<()> {
    let path = root.join(LOCK_FILE);
    if !path.exists() {
        fsutil::write_string(&path, &LockFile::preview_default().to_text())?;
    }
    Ok(())
}

fn load_config(root: &Path) -> Result<Config> {
    let path = root.join(CONFIG_FILE);
    if !path.exists() {
        return Err(LovelyError::Command(format!(
            "{} not found; run lovely init",
            path.display()
        )));
    }
    Config::load_from(&path)
}

fn load_lock(root: &Path) -> Result<LockFile> {
    let path = root.join(LOCK_FILE);
    if !path.exists() {
        return Err(LovelyError::Command(format!(
            "{} not found; run lovely lock",
            path.display()
        )));
    }
    LockFile::load_from(&path)
}

fn unknown_target(target: &str) -> LovelyError {
    LovelyError::Command(format!(
        "unknown target {target:?}; expected web, windows, macos, linux, desktop, or all"
    ))
}

fn github_actions() -> &'static str {
    r#"name: Lovely

on:
  push:
    tags:
      - "v*"
  workflow_dispatch:

jobs:
  web:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo build --release
      - run: ./target/release/lovely lock
      - run: ./target/release/lovely check web
      - run: ./target/release/lovely build web
      - uses: actions/upload-artifact@v4
        with:
          name: lovely-web
          path: dist/*web.zip

  desktop:
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
        include:
          - os: ubuntu-latest
            target: linux
          - os: macos-latest
            target: macos
          - os: windows-latest
            target: windows
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo build --release
      - run: ./target/release/lovely check ${{ matrix.target }}
      - run: ./target/release/lovely build ${{ matrix.target }}
      - uses: actions/upload-artifact@v4
        with:
          name: lovely-${{ matrix.target }}
          path: dist/**
"#
}

fn print_help() {
    println!(
        r#"Lovely — unified LÖVE >= 11 distribution toolchain

Usage:
  lovely init
  lovely lock
  lovely doctor [target]
  lovely check [target...]
  lovely runtime <fetch|doctor|list|cache-dir>
  lovely build [web|windows|macos|linux|desktop|all]
  lovely publish itch
  lovely ci github

Targets:
  web       Itch.io-ready web package shell using pinned LÖVE runtime metadata
  windows   Steam-ready Windows artifact skeleton
  macos     Steam-ready macOS artifact skeleton
  linux     Steam-ready Linux artifact skeleton
"#
    );
}

fn print_runtime_help() {
    println!(
        r#"Lovely runtime registry

Usage:
  lovely runtime fetch <target> <local-path> [--channel love-11-plus] [--sha256 <hex>]
  lovely runtime doctor [target|all]
  lovely runtime list
  lovely runtime cache-dir

Targets:
  web windows macos linux

Notes:
  `fetch` currently installs a local runtime file or directory into the Lovely
  cache. URL fetching should resolve official upstream or vendor-provided
  runtime artifacts into this same cache; Lovely should not need to host them.
"#
    );
}

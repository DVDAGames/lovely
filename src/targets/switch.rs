use crate::archive;
use crate::check::{Diagnostic, DiagnosticReport, Severity};
use crate::config::Config;
use crate::fsutil;
use crate::lockfile::LockFile;
use crate::runtime::{RuntimeKind, RuntimeRegistry};
use crate::targets::{BuildOutput, TargetAdapter};
use crate::{LovelyError, Result};
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::process::Command;

pub struct SwitchAdapter;

impl TargetAdapter for SwitchAdapter {
    fn name(&self) -> &'static str {
        "switch"
    }

    fn doctor(&self, root: &Path, config: &Config, lock: &LockFile) -> Result<DiagnosticReport> {
        let mut report = DiagnosticReport::default();
        let switch = &config.targets.switch;
        if !config.targets.switch.enabled {
            report.push(Diagnostic {
                id: "switch.disabled",
                severity: Severity::Warning,
                message: "switch target is disabled in lovely.toml".to_string(),
                path: None,
            });
        }

        match switch.mode.as_str() {
            "bundler" => doctor_bundler(root, config, lock, &mut report)?,
            "fuse" => doctor_fuse(root, config, &mut report),
            mode => report.push(Diagnostic {
                id: "switch.mode",
                severity: Severity::Error,
                message: format!("unsupported Switch mode {mode:?}; expected bundler or fuse"),
                path: None,
            }),
        }
        Ok(report)
    }

    fn build(&self, root: &Path, config: &Config, lock: &LockFile) -> Result<BuildOutput> {
        let report = self.doctor(root, config, lock)?;
        if report.has_errors() {
            return Err(LovelyError::Command(format!(
                "switch build is not ready:\n{}",
                report.render()
            )));
        }

        let source = root.join(&config.paths.source);
        let output = root.join(&config.paths.output).join("switch");
        fsutil::ensure_dir(&output)?;

        let love_path = output.join(format!("{}.love", config.game.id));
        archive::create_love_archive(&source, &love_path)?;

        let nro_path = output.join(format!("{}.nro", config.game.id));
        match config.targets.switch.mode.as_str() {
            "bundler" => build_bundler(root, config, lock, &love_path, &nro_path)?,
            "fuse" => {
                let runtime = root.join(config.targets.switch.lovepotion_nro.as_ref().unwrap());
                fuse_nro(&runtime, &love_path, &nro_path)?;
            }
            mode => {
                return Err(LovelyError::Command(format!(
                    "unsupported Switch mode {mode:?}; expected bundler or fuse"
                )));
            }
        }

        let license = output.join("LICENSES-Lovely-Switch.txt");
        fsutil::write_string(
            &license,
            &format!(
                "Lovely Switch artifact\nGame: {}\nLÖVE Potion: {} @ {}\n\nThis target is Nintendo Switch homebrew only. Lovely does not produce NSP/eShop/Nintendo SDK commercial packages.\n",
                config.game.name, lock.lovepotion.source, lock.lovepotion.revision
            ),
        )?;

        Ok(BuildOutput {
            target: self.name().to_string(),
            artifacts: vec![love_path, nro_path, license],
        })
    }
}

fn doctor_bundler(
    root: &Path,
    config: &Config,
    lock: &LockFile,
    report: &mut DiagnosticReport,
) -> Result<()> {
    let switch = &config.targets.switch;
    if !fsutil::executable_in_path("elf2nro") {
        report.push(Diagnostic {
            id: "switch.elf2nro",
            severity: Severity::Error,
            message: "elf2nro was not found on PATH; install devkitPro switch-tools for metadata-bearing NRO builds.".to_string(),
            path: None,
        });
    }
    if !fsutil::executable_in_path("nacptool") {
        report.push(Diagnostic {
            id: "switch.nacptool",
            severity: Severity::Error,
            message:
                "nacptool was not found on PATH; install devkitPro switch-tools for NACP metadata."
                    .to_string(),
            path: None,
        });
    }
    if resolve_lovepotion_elf(root, config, lock)?.is_none() {
        match &switch.lovepotion_elf {
            Some(path) => report.push(Diagnostic {
                id: "switch.runtime_missing",
                severity: Severity::Error,
                message: "configured LÖVE Potion .elf does not exist".to_string(),
                path: Some(root.join(path)),
            }),
            None => report.push(Diagnostic {
                id: "switch.runtime_missing",
                severity: Severity::Error,
                message: format!(
                    "targets.switch.lovepotion_elf is required for mode = \"bundler\", or install one with lovely runtime fetch switch <path> for {}.",
                    lock.runtime_channel
                ),
                path: None,
            }),
        }
    }

    if switch.icon.is_none() {
        report.push(Diagnostic {
            id: "switch.icon_missing",
            severity: Severity::Warning,
            message: "no Switch icon configured; elf2nro will use its default behavior."
                .to_string(),
            path: None,
        });
    } else if let Some(icon) = switch.icon.as_ref().map(|path| root.join(path)) {
        if !icon.is_file() {
            report.push(Diagnostic {
                id: "switch.icon_missing",
                severity: Severity::Error,
                message: "configured Switch icon does not exist".to_string(),
                path: Some(icon),
            });
        } else if !is_jpeg(&icon) {
            report.push(Diagnostic {
                id: "switch.icon_format",
                severity: Severity::Warning,
                message: "elf2nro expects a JPEG icon; automatic PNG conversion is a future Lovely runtime-tooling step.".to_string(),
                path: Some(icon),
            });
        }
    }
    Ok(())
}

fn doctor_fuse(root: &Path, config: &Config, report: &mut DiagnosticReport) {
    report.push(Diagnostic {
        id: "switch.fuse_metadata",
        severity: Severity::Warning,
        message: "mode = \"fuse\" preserves metadata from the base LÖVE Potion .nro; use mode = \"bundler\" for custom title/icon metadata.".to_string(),
        path: None,
    });

    match &config.targets.switch.lovepotion_nro {
        Some(path) if root.join(path).is_file() => {}
        Some(path) => report.push(Diagnostic {
            id: "switch.runtime_missing",
            severity: Severity::Error,
            message: "configured LÖVE Potion .nro does not exist".to_string(),
            path: Some(root.join(path)),
        }),
        None => report.push(Diagnostic {
            id: "switch.runtime_missing",
            severity: Severity::Error,
            message: "targets.switch.lovepotion_nro is required for mode = \"fuse\".".to_string(),
            path: None,
        }),
    }
}

fn build_bundler(
    root: &Path,
    config: &Config,
    lock: &LockFile,
    love_path: &Path,
    nro_path: &Path,
) -> Result<()> {
    let output = nro_path
        .parent()
        .ok_or_else(|| LovelyError::Command("invalid Switch output path".to_string()))?;
    let nacp_path = output.join(format!("{}.nacp", config.game.id));
    let base_nro_path = output.join(format!("{}-base.nro", config.game.id));
    let elf_path = resolve_lovepotion_elf(root, config, lock)?.ok_or_else(|| {
        LovelyError::Command(
            "no LÖVE Potion .elf configured or cached for Switch bundler mode".to_string(),
        )
    })?;

    run_command(
        Command::new("nacptool")
            .arg("--create")
            .arg(&config.game.name)
            .arg(&config.game.author)
            .arg(&config.game.version)
            .arg(&nacp_path),
        "nacptool",
    )?;

    let mut elf2nro = Command::new("elf2nro");
    elf2nro.arg(&elf_path).arg(&base_nro_path);
    elf2nro.arg(format!("--nacp={}", nacp_path.display()));
    if let Some(icon) = &config.targets.switch.icon {
        elf2nro.arg(format!("--icon={}", root.join(icon).display()));
    }
    run_command(&mut elf2nro, "elf2nro")?;

    fuse_nro(&base_nro_path, love_path, nro_path)
}

fn resolve_lovepotion_elf(
    root: &Path,
    config: &Config,
    lock: &LockFile,
) -> Result<Option<std::path::PathBuf>> {
    if let Some(path) = &config.targets.switch.lovepotion_elf {
        let path = root.join(path);
        if path.is_file() {
            return Ok(Some(path));
        }
    }

    let Some(runtime) = RuntimeRegistry::new().find("switch", &lock.runtime_channel)? else {
        return Ok(None);
    };
    match runtime.manifest.kind {
        RuntimeKind::File => {
            if runtime.path.extension().and_then(|ext| ext.to_str()) == Some("elf") {
                Ok(Some(runtime.path))
            } else {
                Ok(None)
            }
        }
        RuntimeKind::Directory => {
            let candidate = runtime.path.join("lovepotion.elf");
            if candidate.is_file() {
                Ok(Some(candidate))
            } else {
                Ok(None)
            }
        }
    }
}

fn run_command(command: &mut Command, name: &str) -> Result<()> {
    let output = command.output().map_err(LovelyError::plain_io)?;
    if output.status.success() {
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(LovelyError::Command(format!(
        "{name} failed with status {}\nstdout:\n{}\nstderr:\n{}",
        output.status, stdout, stderr
    )))
}

fn is_jpeg(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("jpg" | "jpeg")
    )
}

fn fuse_nro(runtime: &Path, game: &Path, output: &Path) -> Result<()> {
    let mut out = File::create(output).map_err(|err| LovelyError::io(output, err))?;
    let runtime_bytes = std::fs::read(runtime).map_err(|err| LovelyError::io(runtime, err))?;
    let game_bytes = std::fs::read(game).map_err(|err| LovelyError::io(game, err))?;
    out.write_all(&runtime_bytes)
        .map_err(LovelyError::plain_io)?;
    out.write_all(&game_bytes).map_err(LovelyError::plain_io)?;
    Ok(())
}

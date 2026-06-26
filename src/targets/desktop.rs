use crate::Result;
use crate::archive::{self, ArchiveEntry};
use crate::check::{Diagnostic, DiagnosticReport, Severity};
use crate::config::{Config, DesktopTargetConfig};
use crate::fsutil;
use crate::lockfile::LockFile;
use crate::runtime::{RuntimeKind, RuntimeRegistry};
use crate::targets::{BuildOutput, TargetAdapter};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopPlatform {
    Windows,
    Macos,
    Linux,
}

pub struct DesktopAdapter {
    platform: DesktopPlatform,
}

impl DesktopAdapter {
    pub fn new(platform: DesktopPlatform) -> Self {
        Self { platform }
    }

    fn config<'a>(&self, config: &'a Config) -> &'a DesktopTargetConfig {
        match self.platform {
            DesktopPlatform::Windows => &config.targets.windows,
            DesktopPlatform::Macos => &config.targets.macos,
            DesktopPlatform::Linux => &config.targets.linux,
        }
    }

    fn slug(&self) -> &'static str {
        match self.platform {
            DesktopPlatform::Windows => "windows",
            DesktopPlatform::Macos => "macos",
            DesktopPlatform::Linux => "linux",
        }
    }

    fn artifact_name(&self, game_id: &str) -> String {
        match self.platform {
            DesktopPlatform::Windows => format!("{game_id}-windows-x64.zip"),
            DesktopPlatform::Macos => format!("{game_id}-macos-universal.app.zip"),
            DesktopPlatform::Linux => format!("{game_id}-linux-x86_64.tar"),
        }
    }
}

impl TargetAdapter for DesktopAdapter {
    fn name(&self) -> &'static str {
        self.slug()
    }

    fn doctor(&self, _root: &Path, config: &Config, lock: &LockFile) -> Result<DiagnosticReport> {
        let mut report = DiagnosticReport::default();
        let target_config = self.config(config);
        if !target_config.enabled {
            report.push(Diagnostic {
                id: "desktop.disabled",
                severity: Severity::Warning,
                message: format!("{} target is disabled in lovely.toml", self.slug()),
                path: None,
            });
        }
        if target_config.runtime_archive.is_none()
            && RuntimeRegistry::new()
                .find(self.slug(), &lock.runtime_channel)?
                .is_none()
        {
            report.push(Diagnostic {
                id: "runtime.missing",
                severity: Severity::Warning,
                message: format!(
                    "{} has no pinned runtime configured or cached; build will emit a depot-ready skeleton around game.love. Run lovely runtime fetch {} <path> to install one.",
                    self.slug(),
                    self.slug()
                ),
                path: None,
            });
        }
        if lock.runtime_channel != "12-preview" {
            report.push(Diagnostic {
                id: "runtime.channel",
                severity: Severity::Warning,
                message: format!(
                    "expected runtime channel 12-preview, found {}",
                    lock.runtime_channel
                ),
                path: None,
            });
        }
        Ok(report)
    }

    fn build(&self, root: &Path, config: &Config, lock: &LockFile) -> Result<BuildOutput> {
        let source = root.join(&config.paths.source);
        let base = root.join(&config.paths.output);
        let work = base.join(self.slug());
        fsutil::ensure_dir(&work)?;

        let love_path = work.join(format!("{}.love", config.game.id));
        archive::create_love_archive(&source, &love_path)?;

        let cached_runtime = RuntimeRegistry::new().find(self.slug(), &lock.runtime_channel)?;
        let runtime_available =
            self.config(config).runtime_archive.is_some() || cached_runtime.is_some();
        let readme = desktop_readme(self.slug(), config, lock, runtime_available);
        let mut entries = vec![
            ArchiveEntry::file(
                format!("{}/{}.love", config.game.id, config.game.id),
                std::fs::read(&love_path).map_err(|err| crate::LovelyError::io(&love_path, err))?,
            )?,
            ArchiveEntry::file(
                format!("{}/README-Lovely.txt", config.game.id),
                readme.into_bytes(),
            )?,
        ];

        if let Some(runtime) = &self.config(config).runtime_archive {
            let runtime_path = root.join(runtime);
            append_runtime_entries(&runtime_path, &config.game.id, &mut entries)?;
        } else if let Some(runtime) = &cached_runtime {
            append_cached_runtime_entries(runtime, &config.game.id, &mut entries)?;
        }

        entries.sort_by(|a, b| a.name.cmp(&b.name));
        let artifact = base.join(self.artifact_name(&config.game.id));
        match self.platform {
            DesktopPlatform::Linux => archive::write_tar(&artifact, &entries)?,
            DesktopPlatform::Windows | DesktopPlatform::Macos => {
                archive::write_zip(&artifact, &entries)?
            }
        }

        let steam_dir = base.join("steam").join(self.slug());
        fsutil::ensure_dir(&steam_dir)?;
        fsutil::copy_file(
            &love_path,
            &steam_dir.join(format!("{}.love", config.game.id)),
        )?;
        if let Some(runtime) = &self.config(config).runtime_archive {
            copy_runtime_to_depot(&root.join(runtime), &steam_dir)?;
        } else if let Some(runtime) = &cached_runtime {
            copy_cached_runtime_to_depot(runtime, &steam_dir)?;
        }
        fsutil::write_string(
            &steam_dir.join("depot_build.vdf"),
            &depot_vdf(self.slug(), config, &steam_dir),
        )?;
        fsutil::write_string(&base.join("steam").join("app_build.vdf"), &app_vdf(config))?;

        Ok(BuildOutput {
            target: self.slug().to_string(),
            artifacts: vec![love_path, artifact, steam_dir.join("depot_build.vdf")],
        })
    }
}

fn append_runtime_entries(
    path: &Path,
    game_id: &str,
    entries: &mut Vec<ArchiveEntry>,
) -> Result<()> {
    if path.is_dir() {
        for file in fsutil::collect_files(path)? {
            let rel = fsutil::relative_path(path, &file)?;
            entries.push(ArchiveEntry::file(
                format!("{game_id}/{}", fsutil::normalize_slashes(&rel)),
                std::fs::read(&file).map_err(|err| crate::LovelyError::io(&file, err))?,
            )?);
        }
    } else if path.is_file() {
        let name = path
            .file_name()
            .ok_or_else(|| crate::LovelyError::Command("runtime file has no name".to_string()))?
            .to_string_lossy();
        entries.push(ArchiveEntry::file(
            format!("{game_id}/runtime/{name}"),
            std::fs::read(path).map_err(|err| crate::LovelyError::io(path, err))?,
        )?);
    }
    Ok(())
}

fn append_cached_runtime_entries(
    runtime: &crate::runtime::CachedRuntime,
    game_id: &str,
    entries: &mut Vec<ArchiveEntry>,
) -> Result<()> {
    match runtime.manifest.kind {
        RuntimeKind::Directory => append_runtime_entries(&runtime.path, game_id, entries),
        RuntimeKind::File => append_runtime_entries(&runtime.path, game_id, entries),
    }
}

fn copy_runtime_to_depot(runtime: &Path, depot: &Path) -> Result<()> {
    if runtime.is_dir() {
        fsutil::copy_dir_contents(runtime, depot)?;
    } else if runtime.is_file() {
        let name = runtime
            .file_name()
            .ok_or_else(|| crate::LovelyError::Command("runtime file has no name".to_string()))?;
        fsutil::copy_file(runtime, &depot.join("runtime").join(name))?;
    }
    Ok(())
}

fn copy_cached_runtime_to_depot(
    runtime: &crate::runtime::CachedRuntime,
    depot: &Path,
) -> Result<()> {
    copy_runtime_to_depot(&runtime.path, depot)
}

fn desktop_readme(
    target: &str,
    config: &Config,
    lock: &LockFile,
    runtime_available: bool,
) -> String {
    let runtime_note = if runtime_available {
        "This artifact includes configured or cached LÖVE runtime content. Platform-specific final fusion/signing is still target-runtime dependent."
    } else {
        "This skeleton artifact contains the normalized .love archive. Install a pinned LÖVE runtime with lovely runtime fetch before release builds."
    };
    format!(
        "{name} {version}\nTarget: {target}\nRuntime channel: {channel}\n\n{runtime_note}\n",
        name = config.game.name,
        version = config.game.version,
        target = target,
        channel = lock.runtime_channel,
        runtime_note = runtime_note
    )
}

fn depot_vdf(target: &str, config: &Config, steam_dir: &Path) -> String {
    let depot_id = match target {
        "windows" => config.steam.windows_depot_id.as_deref(),
        "macos" => config.steam.macos_depot_id.as_deref(),
        "linux" => config.steam.linux_depot_id.as_deref(),
        _ => None,
    }
    .unwrap_or("TODO_DEPOT_ID");

    format!(
        r#""DepotBuildConfig"
{{
  "DepotID" "{depot_id}"
  "ContentRoot" "{content_root}"
  "FileMapping"
  {{
    "LocalPath" "*"
    "DepotPath" "."
    "recursive" "1"
  }}
}}
"#,
        depot_id = depot_id,
        content_root = steam_dir.display()
    )
}

fn app_vdf(config: &Config) -> String {
    format!(
        r#""AppBuild"
{{
  "AppID" "{app_id}"
  "Desc" "{name} {version} generated by Lovely"
  "BuildOutput" "steam-output"
  "ContentRoot" "."
  "SetLive" ""
  "Depots"
  {{
    "{windows}" "windows/depot_build.vdf"
    "{macos}" "macos/depot_build.vdf"
    "{linux}" "linux/depot_build.vdf"
  }}
}}
"#,
        app_id = config.steam.app_id.as_deref().unwrap_or("TODO_APP_ID"),
        name = config.game.name,
        version = config.game.version,
        windows = config
            .steam
            .windows_depot_id
            .as_deref()
            .unwrap_or("TODO_WINDOWS_DEPOT_ID"),
        macos = config
            .steam
            .macos_depot_id
            .as_deref()
            .unwrap_or("TODO_MACOS_DEPOT_ID"),
        linux = config
            .steam
            .linux_depot_id
            .as_deref()
            .unwrap_or("TODO_LINUX_DEPOT_ID"),
    )
}

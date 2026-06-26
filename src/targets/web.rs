use crate::Result;
use crate::archive::{self, ArchiveEntry};
use crate::check::{Diagnostic, DiagnosticReport, Severity};
use crate::config::Config;
use crate::fsutil;
use crate::lockfile::LockFile;
use crate::runtime::{RuntimeKind, RuntimeRegistry};
use crate::targets::{BuildOutput, TargetAdapter};
use std::path::Path;

pub struct WebAdapter;

impl TargetAdapter for WebAdapter {
    fn name(&self) -> &'static str {
        "web"
    }

    fn doctor(&self, _root: &Path, config: &Config, lock: &LockFile) -> Result<DiagnosticReport> {
        let mut report = DiagnosticReport::default();
        if !config.targets.web.enabled {
            report.push(Diagnostic {
                id: "web.disabled",
                severity: Severity::Warning,
                message: "web target is disabled in lovely.toml".to_string(),
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
        if lock.has_unresolved_checksums() {
            report.push(Diagnostic {
                id: "lock.unresolved",
                severity: Severity::Warning,
                message: "lovely.lock still contains unresolved runtime checksums; install pinned runtime artifacts before release builds.".to_string(),
                path: None,
            });
        }
        if RuntimeRegistry::new()
            .find("web", &lock.runtime_channel)?
            .is_none()
        {
            report.push(Diagnostic {
                id: "runtime.missing",
                severity: Severity::Warning,
                message: format!(
                    "no cached web runtime for {}; run lovely runtime fetch web <path>",
                    lock.runtime_channel
                ),
                path: None,
            });
        }
        if config.targets.web.variant == "web-threaded" {
            report.push(Diagnostic {
                id: "web.cross_origin_isolation",
                severity: Severity::Warning,
                message: "web-threaded builds require cross-origin isolation headers; Itch.io generally needs web-compat.".to_string(),
                path: None,
            });
        }
        Ok(report)
    }

    fn build(&self, root: &Path, config: &Config, lock: &LockFile) -> Result<BuildOutput> {
        let source = root.join(&config.paths.source);
        let output = root.join(&config.paths.output).join("web");
        fsutil::ensure_dir(&output)?;
        let love_path = output.join("game.love");
        archive::create_love_archive(&source, &love_path)?;

        let index = config
            .targets
            .web
            .html_template
            .as_ref()
            .map(|template| fsutil::read_to_string(&root.join(template)))
            .transpose()?
            .unwrap_or_else(|| default_index(config));
        fsutil::write_string(&output.join("index.html"), &index)?;
        fsutil::write_string(
            &output.join("lovely-runtime.txt"),
            &runtime_manifest(config, lock),
        )?;

        let cached_runtime = RuntimeRegistry::new().find("web", &lock.runtime_channel)?;
        if let Some(runtime) = &cached_runtime {
            copy_runtime_into_output(runtime, &output)?;
        }

        let mut zip_entries = vec![
            ArchiveEntry::file("index.html", index.into_bytes())?,
            ArchiveEntry::file(
                "game.love",
                std::fs::read(&love_path).map_err(|err| crate::LovelyError::io(&love_path, err))?,
            )?,
            ArchiveEntry::file(
                "lovely-runtime.txt",
                runtime_manifest(config, lock).into_bytes(),
            )?,
        ];
        if let Some(runtime) = &cached_runtime {
            append_runtime_zip_entries(runtime, &mut zip_entries)?;
        }
        let upload_zip = root
            .join(&config.paths.output)
            .join(format!("{}-web.zip", config.game.id));
        archive::write_zip(&upload_zip, &zip_entries)?;

        Ok(BuildOutput {
            target: self.name().to_string(),
            artifacts: vec![output.join("index.html"), love_path, upload_zip],
        })
    }
}

fn copy_runtime_into_output(runtime: &crate::runtime::CachedRuntime, output: &Path) -> Result<()> {
    match runtime.manifest.kind {
        RuntimeKind::Directory => {
            for file in fsutil::collect_files(&runtime.path)? {
                let rel = fsutil::relative_path(&runtime.path, &file)?;
                if rel == Path::new("game.love") || rel == Path::new("lovely-runtime.txt") {
                    continue;
                }
                if rel == Path::new("index.html") {
                    continue;
                }
                fsutil::copy_file(&file, &output.join(rel))?;
            }
        }
        RuntimeKind::File => {
            let name = runtime.path.file_name().ok_or_else(|| {
                crate::LovelyError::Command("cached runtime has no file name".to_string())
            })?;
            fsutil::copy_file(&runtime.path, &output.join(name))?;
        }
    }
    Ok(())
}

fn append_runtime_zip_entries(
    runtime: &crate::runtime::CachedRuntime,
    entries: &mut Vec<ArchiveEntry>,
) -> Result<()> {
    match runtime.manifest.kind {
        RuntimeKind::Directory => {
            for file in fsutil::collect_files(&runtime.path)? {
                let rel = fsutil::relative_path(&runtime.path, &file)?;
                if rel == Path::new("game.love") || rel == Path::new("lovely-runtime.txt") {
                    continue;
                }
                if rel == Path::new("index.html") {
                    continue;
                }
                entries.push(ArchiveEntry::file(
                    fsutil::normalize_slashes(&rel),
                    std::fs::read(&file).map_err(|err| crate::LovelyError::io(&file, err))?,
                )?);
            }
        }
        RuntimeKind::File => {
            let name = runtime
                .path
                .file_name()
                .ok_or_else(|| {
                    crate::LovelyError::Command("cached runtime has no file name".to_string())
                })?
                .to_string_lossy()
                .to_string();
            entries.push(ArchiveEntry::file(
                name,
                std::fs::read(&runtime.path)
                    .map_err(|err| crate::LovelyError::io(&runtime.path, err))?,
            )?);
        }
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(())
}

fn default_index(config: &Config) -> String {
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{title}</title>
  <style>
    html, body {{ margin: 0; height: 100%; background: #111; color: #eee; font-family: system-ui, sans-serif; }}
    main {{ min-height: 100%; display: grid; place-items: center; text-align: center; padding: 2rem; box-sizing: border-box; }}
    code {{ color: #a7f3d0; }}
  </style>
</head>
<body>
  <main>
    <div>
      <h1>{title}</h1>
      <p>This is a Lovely web package shell for <code>game.love</code>.</p>
      <p>Install a pinned web runtime with <code>lovely runtime fetch web &lt;path&gt;</code> to include real JavaScript/WASM runtime files.</p>
    </div>
  </main>
</body>
</html>
"#,
        title = html_escape(&config.game.name)
    )
}

fn runtime_manifest(config: &Config, lock: &LockFile) -> String {
    format!(
        "target=web\nvariant={}\nruntime_channel={}\nlove_revision={}\nemscripten_revision={}\nmemory_bytes={}\n",
        config.targets.web.variant,
        lock.runtime_channel,
        lock.love.revision,
        lock.emscripten.revision,
        config.targets.web.memory_bytes,
    )
}

fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

use crate::Result;
use crate::archive::{self, ArchiveEntry};
use crate::check::{Diagnostic, DiagnosticReport, Severity};
use crate::config::Config;
use crate::fsutil;
use crate::lockfile::LockFile;
use crate::runtime::{DEFAULT_CHANNEL, RuntimeKind, RuntimeRegistry};
use crate::targets::{BuildOutput, TargetAdapter};
use std::path::Path;

pub struct WebAdapter;

impl TargetAdapter for WebAdapter {
    fn name(&self) -> &'static str {
        "web"
    }

    fn doctor(&self, root: &Path, config: &Config, lock: &LockFile) -> Result<DiagnosticReport> {
        let mut report = DiagnosticReport::default();
        if !config.targets.web.enabled {
            report.push(Diagnostic {
                id: "web.disabled",
                severity: Severity::Warning,
                message: "web target is disabled in lovely.toml".to_string(),
                path: None,
            });
        }
        if lock.runtime_channel != DEFAULT_CHANNEL {
            report.push(Diagnostic {
                id: "runtime.channel",
                severity: Severity::Warning,
                message: format!(
                    "expected runtime channel {}, found {}",
                    DEFAULT_CHANNEL, lock.runtime_channel
                ),
                path: None,
            });
        }
        let configured_runtime = config.targets.web.runtime_path.as_ref();
        if lock.has_unresolved_checksums() && configured_runtime.is_none() {
            report.push(Diagnostic {
                id: "lock.unresolved",
                severity: Severity::Warning,
                message: "lovely.lock still contains unresolved runtime checksums; install pinned runtime artifacts before release builds.".to_string(),
                path: None,
            });
        }
        if let Some(runtime_path) = configured_runtime {
            let runtime_path = root.join(runtime_path);
            if !runtime_path.exists() {
                report.push(Diagnostic {
                    id: "runtime.missing",
                    severity: Severity::Error,
                    message: format!(
                        "configured web runtime_path does not exist: {}",
                        runtime_path.display()
                    ),
                    path: Some(runtime_path),
                });
            }
        } else if RuntimeRegistry::new()
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
        archive::create_love_archive(
            &source,
            &love_path,
            &config.paths.includes,
            &config.paths.excludes,
        )?;

        let runtime = selected_web_runtime(root, config, lock)?;
        let configured_index_template = config
            .targets
            .web
            .html_template
            .as_ref()
            .map(|template| fsutil::read_to_string(&root.join(template)))
            .transpose()?;
        let index_template = if let Some(template) = configured_index_template {
            template
        } else if let Some(runtime) = &runtime {
            runtime_default_html_template(runtime)?.unwrap_or_else(|| default_index(config))
        } else {
            default_index(config)
        };
        let index = render_html_template(&index_template, config);
        fsutil::write_string(&output.join("index.html"), &index)?;
        fsutil::write_string(
            &output.join("lovely-runtime.txt"),
            &runtime_manifest(config, lock),
        )?;

        if let Some(runtime) = &runtime {
            copy_runtime_into_output(runtime.kind, &runtime.path, &output)?;
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
        if let Some(runtime) = &runtime {
            append_runtime_zip_entries(runtime.kind, &runtime.path, &mut zip_entries)?;
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

struct WebRuntime {
    kind: RuntimeKind,
    path: std::path::PathBuf,
}

fn configured_web_runtime(root: &Path, config: &Config) -> Result<Option<WebRuntime>> {
    let Some(path) = &config.targets.web.runtime_path else {
        return Ok(None);
    };
    let path = root.join(path);
    if !path.exists() {
        return Err(crate::LovelyError::Command(format!(
            "configured web runtime path does not exist: {}",
            path.display()
        )));
    }
    Ok(Some(WebRuntime {
        kind: if path.is_dir() {
            RuntimeKind::Directory
        } else {
            RuntimeKind::File
        },
        path,
    }))
}

fn selected_web_runtime(
    root: &Path,
    config: &Config,
    lock: &LockFile,
) -> Result<Option<WebRuntime>> {
    if let Some(runtime) = configured_web_runtime(root, config)? {
        return Ok(Some(runtime));
    }
    Ok(RuntimeRegistry::new()
        .find("web", &lock.runtime_channel)?
        .map(|runtime| WebRuntime {
            kind: runtime.manifest.kind,
            path: runtime.path,
        }))
}

fn copy_runtime_into_output(kind: RuntimeKind, path: &Path, output: &Path) -> Result<()> {
    match kind {
        RuntimeKind::Directory => {
            for file in fsutil::collect_files(path)? {
                let rel = fsutil::relative_path(path, &file)?;
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
            let name = path.file_name().ok_or_else(|| {
                crate::LovelyError::Command("cached runtime has no file name".to_string())
            })?;
            fsutil::copy_file(path, &output.join(name))?;
        }
    }
    Ok(())
}

fn append_runtime_zip_entries(
    kind: RuntimeKind,
    path: &Path,
    entries: &mut Vec<ArchiveEntry>,
) -> Result<()> {
    match kind {
        RuntimeKind::Directory => {
            for file in fsutil::collect_files(path)? {
                let rel = fsutil::relative_path(path, &file)?;
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
            let name = path
                .file_name()
                .ok_or_else(|| {
                    crate::LovelyError::Command("cached runtime has no file name".to_string())
                })?
                .to_string_lossy()
                .to_string();
            entries.push(ArchiveEntry::file(
                name,
                std::fs::read(path).map_err(|err| crate::LovelyError::io(path, err))?,
            )?);
        }
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(())
}

fn runtime_default_html_template(runtime: &WebRuntime) -> Result<Option<String>> {
    if runtime.kind != RuntimeKind::Directory {
        return Ok(None);
    }

    let manifest_path = runtime.path.join("lovely-runtime.json");
    if !manifest_path.is_file() {
        return Ok(None);
    }

    let manifest = fsutil::read_to_string(&manifest_path)?;
    let Some(html_path) = json_string_field(&manifest, "html")? else {
        return Err(crate::LovelyError::Config(format!(
            "runtime manifest {} is missing html",
            manifest_path.display()
        )));
    };
    let html_path = Path::new(&html_path);
    if html_path.is_absolute()
        || html_path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err(crate::LovelyError::Config(format!(
            "runtime manifest {} has unsafe html path: {}",
            manifest_path.display(),
            html_path.display()
        )));
    }

    Ok(Some(fsutil::read_to_string(&runtime.path.join(html_path))?))
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
    html, body {{ margin: 0; min-height: 100%; background: #111; color: #eee; font-family: system-ui, sans-serif; }}
    body {{ display: grid; min-height: 100vh; }}
    main {{ min-height: 100vh; display: grid; grid-template-rows: auto 1fr auto; gap: 1rem; padding: 1rem; box-sizing: border-box; }}
    header, footer {{ text-align: center; }}
    #game-container {{ min-height: 0; display: grid; place-items: center; }}
    canvas {{ max-width: 100%; max-height: calc(100vh - 9rem); background: #000; image-rendering: pixelated; }}
    button {{ appearance: none; border: 1px solid #555; background: #222; color: #eee; padding: .5rem .75rem; border-radius: 4px; cursor: pointer; }}
    code {{ color: #a7f3d0; }}
  </style>
</head>
<body>
  <main>
    <header>
      <h1>{title}</h1>
    </header>
    <section id="game-container">
      <canvas id="canvas" oncontextmenu="event.preventDefault()"></canvas>
    </section>
    <footer>
      <button type="button" id="fullscreen">Fullscreen</button>
      <p>This package includes <code>game.love</code>. Install a pinned web runtime with <code>lovely runtime fetch web &lt;path&gt;</code> to include JavaScript/WASM runtime files.</p>
    </footer>
  </main>
</body>
</html>
"#,
        title = html_escape(&config.game.name)
    )
}

fn runtime_manifest(config: &Config, lock: &LockFile) -> String {
    format!(
        "target=web\nvariant={}\nruntime_channel={}\nlove_revision={}\nemscripten_revision={}\nmemory_bytes={}\narguments={}\n",
        config.targets.web.variant,
        lock.runtime_channel,
        lock.love.revision,
        lock.emscripten.revision,
        config.targets.web.memory_bytes,
        js_string_array(&web_runtime_arguments(config)),
    )
}

fn render_html_template(template: &str, config: &Config) -> String {
    template
        .replace("__GAME_TITLE__", &html_escape(&config.game.name))
        .replace(
            "__WEB_MEMORY__",
            &config.targets.web.memory_bytes.to_string(),
        )
        .replace(
            "__WEB_ARGUMENTS__",
            &js_string_array(&web_runtime_arguments(config)),
        )
}

fn web_runtime_arguments(config: &Config) -> Vec<String> {
    let mut arguments = Vec::with_capacity(config.targets.web.arguments.len() + 1);
    arguments.push("./game.love".to_string());
    arguments.extend(config.targets.web.arguments.iter().cloned());
    arguments
}

fn js_string_array(values: &[String]) -> String {
    let values = values
        .iter()
        .map(|value| js_string_literal(value))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{values}]")
}

fn js_string_literal(input: &str) -> String {
    let mut output = String::from("\"");
    for ch in input.chars() {
        match ch {
            '\\' => output.push_str("\\\\"),
            '"' => output.push_str("\\\""),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            ch if ch.is_control() => output.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => output.push(ch),
        }
    }
    output.push('"');
    output
}

fn json_string_field(text: &str, key: &str) -> Result<Option<String>> {
    let needle = format!("\"{}\"", key);
    let Some(key_index) = text.find(&needle) else {
        return Ok(None);
    };
    let after_key = &text[key_index + needle.len()..];
    let Some(colon_index) = after_key.find(':') else {
        return Err(crate::LovelyError::Config(format!(
            "runtime manifest field {key:?} is missing ':'"
        )));
    };
    let value = after_key[colon_index + 1..].trim_start();
    let Some(value) = value.strip_prefix('"') else {
        return Err(crate::LovelyError::Config(format!(
            "runtime manifest field {key:?} is not a string"
        )));
    };

    let mut output = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => return Ok(Some(output)),
            '\\' => {
                let Some(escaped) = chars.next() else {
                    return Err(crate::LovelyError::Config(format!(
                        "runtime manifest field {key:?} has an incomplete escape"
                    )));
                };
                match escaped {
                    '"' => output.push('"'),
                    '\\' => output.push('\\'),
                    '/' => output.push('/'),
                    'b' => output.push('\u{0008}'),
                    'f' => output.push('\u{000c}'),
                    'n' => output.push('\n'),
                    'r' => output.push('\r'),
                    't' => output.push('\t'),
                    'u' => {
                        let mut hex = String::new();
                        for _ in 0..4 {
                            let Some(digit) = chars.next() else {
                                return Err(crate::LovelyError::Config(format!(
                                    "runtime manifest field {key:?} has an incomplete unicode escape"
                                )));
                            };
                            hex.push(digit);
                        }
                        let code = u32::from_str_radix(&hex, 16).map_err(|_| {
                            crate::LovelyError::Config(format!(
                                "runtime manifest field {key:?} has an invalid unicode escape"
                            ))
                        })?;
                        let Some(decoded) = char::from_u32(code) else {
                            return Err(crate::LovelyError::Config(format!(
                                "runtime manifest field {key:?} has an invalid unicode scalar"
                            )));
                        };
                        output.push(decoded);
                    }
                    other => {
                        return Err(crate::LovelyError::Config(format!(
                            "runtime manifest field {key:?} has an invalid escape: {other}"
                        )));
                    }
                }
            }
            ch => output.push(ch),
        }
    }

    Err(crate::LovelyError::Config(format!(
        "runtime manifest field {key:?} is unterminated"
    )))
}

fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

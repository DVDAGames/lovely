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

        let index_template = config
            .targets
            .web
            .html_template
            .as_ref()
            .map(|template| fsutil::read_to_string(&root.join(template)))
            .transpose()?
            .unwrap_or_else(|| default_index(config));
        let index = render_html_template(&index_template, config);
        fsutil::write_string(&output.join("index.html"), &index)?;
        let shims = web_shims();
        fsutil::write_string(&output.join("lovely-web-shims.js"), shims)?;
        fsutil::write_string(
            &output.join("lovely-runtime.txt"),
            &runtime_manifest(config, lock),
        )?;

        let configured_runtime = configured_web_runtime(root, config)?;
        let cached_runtime = if configured_runtime.is_none() {
            RuntimeRegistry::new().find("web", &lock.runtime_channel)?
        } else {
            None
        };
        if let Some(runtime) = &configured_runtime {
            copy_runtime_into_output(runtime.kind, &runtime.path, &output)?;
        } else if let Some(runtime) = &cached_runtime {
            copy_runtime_into_output(runtime.manifest.kind, &runtime.path, &output)?;
        }

        let mut zip_entries = vec![
            ArchiveEntry::file("index.html", index.into_bytes())?,
            ArchiveEntry::file("lovely-web-shims.js", shims.as_bytes().to_vec())?,
            ArchiveEntry::file(
                "game.love",
                std::fs::read(&love_path).map_err(|err| crate::LovelyError::io(&love_path, err))?,
            )?,
            ArchiveEntry::file(
                "lovely-runtime.txt",
                runtime_manifest(config, lock).into_bytes(),
            )?,
        ];
        if let Some(runtime) = &configured_runtime {
            append_runtime_zip_entries(runtime.kind, &runtime.path, &mut zip_entries)?;
        } else if let Some(runtime) = &cached_runtime {
            append_runtime_zip_entries(runtime.manifest.kind, &runtime.path, &mut zip_entries)?;
        }
        let upload_zip = root
            .join(&config.paths.output)
            .join(format!("{}-web.zip", config.game.id));
        archive::write_zip(&upload_zip, &zip_entries)?;

        Ok(BuildOutput {
            target: self.name().to_string(),
            artifacts: vec![
                output.join("index.html"),
                output.join("lovely-web-shims.js"),
                love_path,
                upload_zip,
            ],
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

fn copy_runtime_into_output(kind: RuntimeKind, path: &Path, output: &Path) -> Result<()> {
    match kind {
        RuntimeKind::Directory => {
            for file in fsutil::collect_files(path)? {
                let rel = fsutil::relative_path(path, &file)?;
                if rel == Path::new("game.love") || rel == Path::new("lovely-runtime.txt") {
                    continue;
                }
                if rel == Path::new("index.html") || rel == Path::new("lovely-web-shims.js") {
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
                if rel == Path::new("index.html") || rel == Path::new("lovely-web-shims.js") {
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
  <script src="lovely-web-shims.js"></script>
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
  <script>
    window.Module = window.Module || {{}};
    window.Module.arguments = __WEB_ARGUMENTS__;
    window.Module.INITIAL_MEMORY = __WEB_MEMORY__;
    LovelyWeb.install({{ canvasId: "canvas", containerId: "game-container" }});
    document.getElementById("fullscreen").addEventListener("click", function () {{
      LovelyWeb.toggleFullscreen("game-container", "canvas");
    }});
  </script>
</body>
</html>
"#,
        title = html_escape(&config.game.name)
    )
}

fn web_shims() -> &'static str {
    r#""use strict";

// Browser-side compatibility helpers for LÖVE web runtimes.
// These are runtime-agnostic: if the active runtime exposes matching hooks,
// Lovely uses them; otherwise the helpers degrade into ordinary DOM behavior.
(function (global) {
  var mobileInput = null;
  var textInputActive = false;
  var touchListener = null;
  var resizeObserver = null;

  function isMobileDevice() {
    return /iPhone|iPad|iPod|Android|webOS|BlackBerry|IEMobile|Opera Mini/i.test(navigator.userAgent);
  }

  function canvasById(canvasId) {
    return document.getElementById(canvasId || "canvas");
  }

  function containerById(containerId, canvas) {
    return document.getElementById(containerId || "game-container") || (canvas && canvas.parentElement) || document.body;
  }

  function createMobileTextInput(canvasId) {
    if (mobileInput) {
      return mobileInput;
    }

    mobileInput = document.createElement("input");
    mobileInput.type = "text";
    mobileInput.autocapitalize = "none";
    mobileInput.autocomplete = "off";
    mobileInput.autocorrect = "off";
    mobileInput.spellcheck = false;
    mobileInput.style.position = "fixed";
    mobileInput.style.left = "0";
    mobileInput.style.top = "0";
    mobileInput.style.width = "1px";
    mobileInput.style.height = "1px";
    mobileInput.style.opacity = "0";
    mobileInput.style.fontSize = "16px";
    mobileInput.style.pointerEvents = "none";
    document.body.appendChild(mobileInput);

    mobileInput.addEventListener("input", function (event) {
      var canvas = canvasById(canvasId);
      if (!canvas || !event.data) {
        return;
      }
      canvas.dispatchEvent(new KeyboardEvent("keypress", {
        key: event.data,
        code: event.data,
        charCode: event.data.charCodeAt(0),
        keyCode: event.data.charCodeAt(0),
        which: event.data.charCodeAt(0),
        bubbles: true
      }));
      mobileInput.value = "";
    });

    return mobileInput;
  }

  function startTextInput(canvasId) {
    var canvas = canvasById(canvasId);
    if (!canvas) {
      return;
    }
    createMobileTextInput(canvasId);
    textInputActive = true;

    if (!isMobileDevice() || touchListener) {
      return;
    }

    touchListener = function () {
      if (textInputActive && mobileInput) {
        mobileInput.focus();
      }
    };
    canvas.addEventListener("touchstart", touchListener, { passive: true });
  }

  function stopTextInput(canvasId) {
    var canvas = canvasById(canvasId);
    textInputActive = false;
    if (mobileInput) {
      mobileInput.blur();
    }
    if (canvas && touchListener) {
      canvas.removeEventListener("touchstart", touchListener);
    }
    touchListener = null;
  }

  function resizeCanvas(canvasId, containerId) {
    var canvas = canvasById(canvasId);
    if (!canvas) {
      return;
    }
    var container = containerById(containerId, canvas);
    var bounds = container.getBoundingClientRect();
    var width = Math.max(1, Math.floor(bounds.width || canvas.clientWidth || canvas.width));
    var height = Math.max(1, Math.floor(bounds.height || canvas.clientHeight || canvas.height));

    if (canvas.width !== width || canvas.height !== height) {
      canvas.width = width;
      canvas.height = height;
      if (global.Module && global.Module.canvas === canvas && typeof global.Module.setCanvasSize === "function") {
        try {
          global.Module.setCanvasSize(width, height);
        } catch (error) {
          console.warn("LovelyWeb resize: Module.setCanvasSize failed", error);
        }
      }
      window.dispatchEvent(new UIEvent("resize"));
    }
  }

  function install(options) {
    options = options || {};
    var canvasId = options.canvasId || "canvas";
    var containerId = options.containerId || "game-container";
    var canvas = canvasById(canvasId);
    if (!canvas) {
      return;
    }

    global.SDL_StartTextInput = global.SDL_StartTextInput || function () {
      startTextInput(canvasId);
    };
    global.SDL_StopTextInput = global.SDL_StopTextInput || function () {
      stopTextInput(canvasId);
    };

    window.addEventListener("resize", function () {
      resizeCanvas(canvasId, containerId);
    });
    document.addEventListener("fullscreenchange", function () {
      resizeCanvas(canvasId, containerId);
    });

    if ("ResizeObserver" in global) {
      resizeObserver = new ResizeObserver(function () {
        resizeCanvas(canvasId, containerId);
      });
      resizeObserver.observe(containerById(containerId, canvas));
    }

    resizeCanvas(canvasId, containerId);
  }

  function toggleFullscreen(containerId, canvasId) {
    var canvas = canvasById(canvasId);
    var container = containerById(containerId, canvas);
    if (isMobileDevice() && canvas) {
      var active = canvas.style.position === "fixed";
      canvas.style.position = active ? "" : "fixed";
      canvas.style.inset = active ? "" : "0";
      canvas.style.width = active ? "" : "100vw";
      canvas.style.height = active ? "" : "100vh";
      canvas.style.maxWidth = active ? "" : "none";
      canvas.style.maxHeight = active ? "" : "none";
      canvas.style.zIndex = active ? "" : "9999";
      document.body.style.overflow = active ? "" : "hidden";
      resizeCanvas(canvasId, containerId);
      return;
    }

    if (document.fullscreenElement) {
      document.exitFullscreen();
    } else if (container.requestFullscreen) {
      container.requestFullscreen();
    }
  }

  global.LovelyWeb = {
    install: install,
    resizeCanvas: resizeCanvas,
    startTextInput: startTextInput,
    stopTextInput: stopTextInput,
    toggleFullscreen: toggleFullscreen
  };
})(window);
"#
}

fn runtime_manifest(config: &Config, lock: &LockFile) -> String {
    format!(
        "target=web\nvariant={}\nruntime_channel={}\nlove_revision={}\nemscripten_revision={}\nmemory_bytes={}\narguments={}\nshims=lovely-web-shims.js\n",
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

fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

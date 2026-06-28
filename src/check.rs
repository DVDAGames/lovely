use crate::config::Config;
use crate::fsutil;
use crate::{LovelyError, Result};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub id: &'static str,
    pub severity: Severity,
    pub message: String,
    pub path: Option<PathBuf>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DiagnosticReport {
    pub diagnostics: Vec<Diagnostic>,
}

impl DiagnosticReport {
    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    pub fn extend(&mut self, other: DiagnosticReport) {
        self.diagnostics.extend(other.diagnostics);
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error)
    }

    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }

    pub fn render(&self) -> String {
        if self.is_empty() {
            return "No Lovely compatibility issues found.\n".to_string();
        }

        let mut out = String::new();
        for diagnostic in &self.diagnostics {
            let severity = match diagnostic.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
            };
            if let Some(path) = &diagnostic.path {
                out.push_str(&format!(
                    "{severity}[{}] {}: {}\n",
                    diagnostic.id,
                    path.display(),
                    diagnostic.message
                ));
            } else {
                out.push_str(&format!(
                    "{severity}[{}] {}\n",
                    diagnostic.id, diagnostic.message
                ));
            }
        }
        out
    }
}

pub fn check_project(root: &Path, config: &Config, targets: &[String]) -> Result<DiagnosticReport> {
    let source = root.join(&config.paths.source);
    if !source.is_dir() {
        return Err(LovelyError::Config(format!(
            "source directory does not exist: {}",
            source.display()
        )));
    }

    let mut report = DiagnosticReport::default();
    let requested = if targets.is_empty() {
        vec!["web".to_string(), "desktop".to_string()]
    } else {
        targets.to_vec()
    };

    let files =
        fsutil::collect_included_files(&source, &config.paths.includes, &config.paths.excludes)?;
    let wants_web = requested
        .iter()
        .any(|target| target == "web" || target == "all");

    for file in files {
        let Some(ext) = file.extension().and_then(|ext| ext.to_str()) else {
            continue;
        };
        if ext != "lua" {
            continue;
        }

        let text = fs::read_to_string(&file).map_err(|err| LovelyError::io(&file, err))?;
        if wants_web {
            check_web_lua(&mut report, config, &file, &text);
        }
    }

    Ok(report)
}

fn check_web_lua(report: &mut DiagnosticReport, config: &Config, path: &Path, text: &str) {
    if text.contains("require(\"ffi\")")
        || text.contains("require 'ffi'")
        || text.contains("require('ffi')")
    {
        push_unless_allowed(
            report,
            config,
            Diagnostic {
                id: "web.ffi",
                severity: Severity::Error,
                message: "LuaJIT FFI cannot run in the web target.".to_string(),
                path: Some(path.to_path_buf()),
            },
        );
    }

    for needle in ["package.loadlib", "io.popen", "os.execute"] {
        if text.contains(needle) {
            push_unless_allowed(
                report,
                config,
                Diagnostic {
                    id: "web.native",
                    severity: Severity::Error,
                    message: format!(
                        "{needle} suggests native process or module usage, which is not web-portable."
                    ),
                    path: Some(path.to_path_buf()),
                },
            );
        }
    }

    if text.contains("unpack(") && !text.contains("table.unpack(") {
        push_unless_allowed(
            report,
            config,
            Diagnostic {
                id: "web.lua52_unpack",
                severity: Severity::Warning,
                message: "some LÖVE web runtimes use Lua 5.2+ semantics; prefer table.unpack over global unpack.".to_string(),
                path: Some(path.to_path_buf()),
            },
        );
    }

    if text.contains("require(\"bit\")")
        || text.contains("require 'bit'")
        || text.contains("require('bit')")
    {
        push_unless_allowed(
            report,
            config,
            Diagnostic {
                id: "web.bit_module",
                severity: Severity::Warning,
                message: "bit may be unavailable in some LÖVE web runtimes; use bit32 or a compatibility shim.".to_string(),
                path: Some(path.to_path_buf()),
            },
        );
    }

    for needle in ["love.audio.play(", "love.audio.stop(", "love.audio.pause("] {
        if text.contains(needle) {
            push_unless_allowed(
                report,
                config,
                Diagnostic {
                    id: "web.module_audio",
                    severity: Severity::Warning,
                    message: format!(
                        "{needle} has crashed in some love.js builds; prefer Source methods like source:play()."
                    ),
                    path: Some(path.to_path_buf()),
                },
            );
        }
    }

    if text.contains("newShader") && text.contains("varying") {
        push_unless_allowed(
            report,
            config,
            Diagnostic {
                id: "web.shader_varying",
                severity: Severity::Warning,
                message:
                    "custom shader varyings may need hoisting for strict WebGL implementations."
                        .to_string(),
                path: Some(path.to_path_buf()),
            },
        );
    }
}

fn push_unless_allowed(report: &mut DiagnosticReport, config: &Config, diagnostic: Diagnostic) {
    if config
        .compatibility
        .allow_warnings
        .iter()
        .any(|allowed| allowed == diagnostic.id)
        && diagnostic.severity == Severity::Warning
    {
        return;
    }
    report.push(diagnostic);
}

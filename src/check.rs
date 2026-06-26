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
        vec![
            "web".to_string(),
            "switch".to_string(),
            "desktop".to_string(),
        ]
    } else {
        targets.to_vec()
    };

    let files = fsutil::collect_files(&source)?;
    let wants_web = requested
        .iter()
        .any(|target| target == "web" || target == "all");
    let wants_switch = requested
        .iter()
        .any(|target| target == "switch" || target == "all");

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
        if wants_switch {
            check_switch_lua(&mut report, config, &file, &text);
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
}

fn check_switch_lua(report: &mut DiagnosticReport, config: &Config, path: &Path, text: &str) {
    for needle in ["newShader", "love.graphics.setShader"] {
        if text.contains(needle) {
            push_unless_allowed(
                report,
                config,
                Diagnostic {
                    id: "switch.shader",
                    severity: Severity::Warning,
                    message: "LÖVE Potion compatibility docs list shader support as unavailable or limited; gate this code per platform.".to_string(),
                    path: Some(path.to_path_buf()),
                },
            );
        }
    }

    for needle in ["love.video", "newVideo"] {
        if text.contains(needle) {
            push_unless_allowed(
                report,
                config,
                Diagnostic {
                    id: "switch.video",
                    severity: Severity::Warning,
                    message: "video APIs are a likely Switch homebrew portability issue."
                        .to_string(),
                    path: Some(path.to_path_buf()),
                },
            );
        }
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

pub mod desktop;
pub mod web;

use crate::Result;
use crate::check::DiagnosticReport;
use crate::config::Config;
use crate::lockfile::LockFile;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildOutput {
    pub target: String,
    pub artifacts: Vec<std::path::PathBuf>,
}

pub trait TargetAdapter {
    fn name(&self) -> &'static str;
    fn doctor(&self, root: &Path, config: &Config, lock: &LockFile) -> Result<DiagnosticReport>;
    fn build(&self, root: &Path, config: &Config, lock: &LockFile) -> Result<BuildOutput>;
}

pub fn adapter_for(target: &str) -> Option<Box<dyn TargetAdapter>> {
    match target {
        "web" => Some(Box::new(web::WebAdapter)),
        "windows" => Some(Box::new(desktop::DesktopAdapter::new(
            desktop::DesktopPlatform::Windows,
        ))),
        "macos" => Some(Box::new(desktop::DesktopAdapter::new(
            desktop::DesktopPlatform::Macos,
        ))),
        "linux" => Some(Box::new(desktop::DesktopAdapter::new(
            desktop::DesktopPlatform::Linux,
        ))),
        _ => None,
    }
}

pub fn expand_targets(target: &str) -> Vec<&'static str> {
    match target {
        "all" => vec!["web", "windows", "macos", "linux"],
        "desktop" => vec!["windows", "macos", "linux"],
        "web" => vec!["web"],
        "windows" => vec!["windows"],
        "macos" => vec!["macos"],
        "linux" => vec!["linux"],
        _ => Vec::new(),
    }
}

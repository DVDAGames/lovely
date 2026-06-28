use crate::runtime::cache_dir;
use crate::{LovelyError, Result};
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const BUTLER_BASE_URL: &str = "https://broth.itch.zone/butler";

#[derive(Debug, Clone)]
pub struct Butler {
    path: PathBuf,
}

impl Butler {
    pub fn resolve() -> Result<Self> {
        if let Some(path) = env::var_os("LOVELY_BUTLER_PATH") {
            let path = PathBuf::from(path);
            if path.is_file() {
                return Ok(Self { path });
            }
            return Err(LovelyError::Command(format!(
                "LOVELY_BUTLER_PATH does not point to a file: {}",
                path.display()
            )));
        }

        if let Some(path) = find_on_path("butler") {
            return Ok(Self { path });
        }

        let path = cached_butler_path();
        if path.is_file() {
            return Ok(Self { path });
        }

        fetch_butler(&path)?;
        Ok(Self { path })
    }

    pub fn push(&self, artifact: &Path, destination: &str) -> Result<()> {
        let status = Command::new(&self.path)
            .arg("push")
            .arg(artifact)
            .arg(destination)
            .status()
            .map_err(|err| LovelyError::io(&self.path, err))?;

        if !status.success() {
            return Err(LovelyError::Command(format!(
                "butler push failed with status {status}"
            )));
        }

        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn fetch_butler(destination: &Path) -> Result<()> {
    let Some(platform) = butler_platform() else {
        return Err(LovelyError::Command(
            "automatic Butler install is not supported on this platform; install Butler or set LOVELY_BUTLER_PATH".to_string(),
        ));
    };

    let Some(parent) = destination.parent() else {
        return Err(LovelyError::Command(
            "invalid Butler cache destination".to_string(),
        ));
    };
    fs::create_dir_all(parent).map_err(|err| LovelyError::io(parent, err))?;

    let archive = parent.join("butler.zip");
    let url = env::var("LOVELY_BUTLER_URL")
        .unwrap_or_else(|_| format!("{BUTLER_BASE_URL}/{platform}/LATEST/archive/default"));

    run_tool(
        "curl",
        &[
            OsString::from("-fsSL"),
            OsString::from("-o"),
            archive.as_os_str().to_os_string(),
            OsString::from(url),
        ],
        "download Butler",
    )?;

    #[cfg(windows)]
    {
        run_tool(
            "powershell",
            &[
                OsString::from("-NoProfile"),
                OsString::from("-Command"),
                OsString::from(format!(
                    "Expand-Archive -Force -LiteralPath '{}' -DestinationPath '{}'",
                    archive.display(),
                    parent.display()
                )),
            ],
            "extract Butler",
        )?;
    }

    #[cfg(not(windows))]
    {
        run_tool(
            "unzip",
            &[
                OsString::from("-o"),
                archive.as_os_str().to_os_string(),
                OsString::from("-d"),
                parent.as_os_str().to_os_string(),
            ],
            "extract Butler",
        )?;
    }

    if !destination.is_file() {
        return Err(LovelyError::Command(format!(
            "downloaded Butler archive did not contain {}",
            destination.display()
        )));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(destination)
            .map_err(|err| LovelyError::io(destination, err))?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(destination, permissions)
            .map_err(|err| LovelyError::io(destination, err))?;
    }

    Ok(())
}

fn run_tool(tool: &str, args: &[OsString], action: &str) -> Result<()> {
    let output = Command::new(tool).args(args).output().map_err(|err| {
        LovelyError::Command(format!(
            "could not {action}: {tool} is required for automatic Butler install ({err})"
        ))
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(LovelyError::Command(format!(
            "could not {action}: {tool} exited with status {}; {}",
            output.status,
            stderr.trim()
        )));
    }

    Ok(())
}

fn cached_butler_path() -> PathBuf {
    cache_dir()
        .join("tools")
        .join("butler")
        .join(butler_platform().unwrap_or("unknown"))
        .join(butler_binary_name())
}

fn find_on_path(binary: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    for dir in env::split_paths(&path) {
        for name in path_binary_names(binary) {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn path_binary_names(binary: &str) -> Vec<String> {
    #[cfg(windows)]
    {
        if Path::new(binary).extension().is_some() {
            return vec![binary.to_string()];
        }
        vec![
            format!("{binary}.exe"),
            format!("{binary}.cmd"),
            format!("{binary}.bat"),
        ]
    }

    #[cfg(not(windows))]
    {
        vec![binary.to_string()]
    }
}

fn butler_binary_name() -> &'static str {
    if cfg!(windows) {
        "butler.exe"
    } else {
        "butler"
    }
}

fn butler_platform() -> Option<&'static str> {
    match (env::consts::OS, env::consts::ARCH) {
        ("linux", "x86_64") => Some("linux-amd64"),
        ("linux", "aarch64") => Some("linux-arm64"),
        ("macos", "x86_64") => Some("darwin-amd64"),
        ("macos", "aarch64") => Some("darwin-arm64"),
        ("windows", "x86_64") => Some("windows-amd64"),
        ("windows", "aarch64") => Some("windows-arm64"),
        _ => None,
    }
}

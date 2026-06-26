use crate::{LovelyError, Result};
use std::fs;
use std::path::{Component, Path, PathBuf};

pub fn ensure_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path).map_err(|err| LovelyError::io(path, err))
}

pub fn read_to_string(path: &Path) -> Result<String> {
    fs::read_to_string(path).map_err(|err| LovelyError::io(path, err))
}

pub fn write_string(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    fs::write(path, contents).map_err(|err| LovelyError::io(path, err))
}

pub fn copy_file(from: &Path, to: &Path) -> Result<u64> {
    if let Some(parent) = to.parent() {
        ensure_dir(parent)?;
    }
    fs::copy(from, to).map_err(|err| LovelyError::io(to, err))
}

pub fn copy_dir_contents(from: &Path, to: &Path) -> Result<()> {
    ensure_dir(to)?;
    for file in collect_files(from)? {
        let rel = relative_path(from, &file)?;
        copy_file(&file, &to.join(rel))?;
    }
    Ok(())
}

pub fn normalize_slashes(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

pub fn relative_path(base: &Path, path: &Path) -> Result<PathBuf> {
    path.strip_prefix(base).map(Path::to_path_buf).map_err(|_| {
        LovelyError::Archive(format!("{} is outside {}", path.display(), base.display()))
    })
}

pub fn executable_in_path(name: &str) -> bool {
    let Some(path_var) = std::env::var_os("PATH") else {
        return false;
    };

    std::env::split_paths(&path_var).any(|dir| {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return true;
        }

        #[cfg(windows)]
        {
            let candidate = dir.join(format!("{name}.exe"));
            candidate.is_file()
        }

        #[cfg(not(windows))]
        {
            false
        }
    })
}

pub fn collect_files(root: &Path) -> Result<Vec<PathBuf>> {
    fn visit(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
        for entry in fs::read_dir(dir).map_err(|err| LovelyError::io(dir, err))? {
            let entry = entry.map_err(LovelyError::plain_io)?;
            let path = entry.path();
            let rel = relative_path(root, &path)?;
            if should_skip(&rel) {
                continue;
            }
            let kind = entry.file_type().map_err(LovelyError::plain_io)?;
            if kind.is_dir() {
                visit(root, &path, out)?;
            } else if kind.is_file() {
                out.push(path);
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    visit(root, root, &mut files)?;
    files.sort_by_key(|path| normalize_slashes(&relative_path(root, path).unwrap_or_default()));
    Ok(files)
}

fn should_skip(rel: &Path) -> bool {
    let first = rel
        .components()
        .next()
        .and_then(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy()),
            _ => None,
        });

    matches!(
        first.as_deref(),
        Some(".git" | ".lovely" | "target" | "dist" | "build")
    )
}

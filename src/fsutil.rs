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
    let normalized = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/");
    if normalized.is_empty() && path == Path::new(".") {
        ".".to_string()
    } else {
        normalized
    }
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

pub fn collect_included_files(
    root: &Path,
    includes: &[String],
    excludes: &[String],
) -> Result<Vec<PathBuf>> {
    let files = collect_files(root)?;
    if includes.is_empty() {
        return Ok(Vec::new());
    }

    let mut included = files
        .into_iter()
        .filter(|file| {
            relative_path(root, file)
                .map(|rel| {
                    let rel = normalize_slashes(&rel);
                    matches_any_pattern(&rel, includes) && !matches_any_pattern(&rel, excludes)
                })
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    included.sort_by_key(|path| normalize_slashes(&relative_path(root, path).unwrap_or_default()));
    Ok(included)
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

fn matches_any_pattern(rel: &str, patterns: &[String]) -> bool {
    patterns
        .iter()
        .map(|pattern| normalize_pattern(pattern))
        .any(|pattern| glob_match(&pattern, rel))
}

fn normalize_pattern(pattern: &str) -> String {
    pattern
        .trim()
        .trim_start_matches("./")
        .trim_matches('/')
        .replace('\\', "/")
}

fn glob_match(pattern: &str, rel: &str) -> bool {
    if pattern.is_empty() {
        return false;
    }
    if pattern == "**" || pattern == "**/*" {
        return true;
    }

    let pattern_parts = pattern.split('/').collect::<Vec<_>>();
    let rel_parts = rel.split('/').collect::<Vec<_>>();
    glob_match_parts(&pattern_parts, &rel_parts)
}

fn glob_match_parts(pattern: &[&str], rel: &[&str]) -> bool {
    match (pattern.first(), rel.first()) {
        (None, None) => true,
        (None, Some(_)) => false,
        (Some(&"**"), _) => {
            glob_match_parts(&pattern[1..], rel)
                || (!rel.is_empty() && glob_match_parts(pattern, &rel[1..]))
        }
        (Some(_), None) => false,
        (Some(pattern_part), Some(rel_part)) => {
            segment_match(pattern_part, rel_part) && glob_match_parts(&pattern[1..], &rel[1..])
        }
    }
}

fn segment_match(pattern: &str, text: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if !pattern.contains('*') {
        return pattern == text;
    }

    let mut remainder = text;
    let mut first = true;
    for part in pattern.split('*') {
        if part.is_empty() {
            continue;
        }
        if first && !pattern.starts_with('*') {
            let Some(next) = remainder.strip_prefix(part) else {
                return false;
            };
            remainder = next;
        } else if let Some(index) = remainder.find(part) {
            remainder = &remainder[index + part.len()..];
        } else {
            return false;
        }
        first = false;
    }

    pattern.ends_with('*') || remainder.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_include_patterns_match_expected_paths() {
        let includes = vec![
            "main.lua".to_string(),
            "conf.lua".to_string(),
            "src/**".to_string(),
            "assets/**/*".to_string(),
            "*.md".to_string(),
        ];

        assert!(matches_any_pattern("main.lua", &includes));
        assert!(matches_any_pattern("src/game/state.lua", &includes));
        assert!(matches_any_pattern("assets/sprites/boat.png", &includes));
        assert!(matches_any_pattern("README.md", &includes));
        assert!(!matches_any_pattern("scripts/release.sh", &includes));
        assert!(!matches_any_pattern("node_modules/pkg/index.js", &includes));
    }
}
